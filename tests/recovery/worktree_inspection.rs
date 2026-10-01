//! Real worktree observations through the native row hover card, its refresh
//! and stale delivery.
use crate::{
    evidence_support::deferred,
    macos::{git, mount, unmount},
};
use gpui::{
    point, px, AnyWindowHandle, Bounds, Entity, Modifiers, Pixels, Point, VisualTestAppContext,
};
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
    KagiApp,
};
use kagi_domain::remove::{WorktreeRemovalVerdict as Verdict, WorktreeUnknownReason};
use kagi_git::worktree_inspection::{inspect_worktree, WorktreeInspection};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

/// Past GPUI's 500ms hoverable-tooltip show and hide delays. The test clock
/// only moves when advanced, so a resting pointer opens nothing on its own.
const TOOLTIP_DELAY: Duration = Duration::from_secs(1);
/// Recorded bounds a frame must re-prove before the card counts as drawn.
const CARD_IDS: [&str; 7] = [
    "worktree-inspection",
    "worktree-inspection-heading",
    "worktree-inspection-body",
    "worktree-inspection-verdict",
    "worktree-inspection-refresh",
    "worktree-inspection-measuring",
    "sidebar-panes",
];

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("tracked.txt"), "committed\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n.env\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let remote = root.join("remote.git");
    git(&root, &["init", "--bare", "-q", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-qu", "origin", "main"]);
    for name in ["pushed", "dirty", "locked"] {
        let path = root.join(name);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                name,
                path.to_str().unwrap(),
                "main",
            ],
        );
    }
    let detached = root.join("detached");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            detached.to_str().unwrap(),
            "main",
        ],
    );
    git(
        &root.join("pushed"),
        &["commit", "--allow-empty", "-qm", "published only"],
    );
    git(&root.join("pushed"), &["push", "-qu", "origin", "pushed"]);
    std::fs::write(root.join("dirty/tracked.txt"), "uncommitted\n").unwrap();
    git(
        &repo,
        &["worktree", "lock", root.join("locked").to_str().unwrap()],
    );
    for name in ["pushed", "dirty", "locked", "detached"] {
        std::fs::create_dir(root.join(name).join("target")).unwrap();
        std::fs::write(root.join(name).join("target/build.bin"), vec![7; 65_536]).unwrap();
    }
    (dir, repo)
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, id: &str) -> Option<Bounds<Pixels>> {
    e2e::control_bounds(window.window_id(), id)
}

/// Draw with the card's bounds cleared, so presence is proven by this frame
/// rather than inherited from an earlier one.
fn redraw_card(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for id in CARD_IDS {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    draw(cx, window);
}

fn card_shown(window: AnyWindowHandle) -> bool {
    bounds(window, "worktree-inspection").is_some()
}

/// The five panes' measured spans and their shared viewport.
fn pane_layout(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> ([(f32, f32); 5], Bounds<Pixels>) {
    let panes = cx.read(|cx| {
        let sidebar = &app.read(cx).sidebar;
        std::array::from_fn(|index| sidebar.pane_geom[index].get())
    });
    (
        panes,
        bounds(window, "sidebar-panes").expect("pane viewport"),
    )
}

/// Rest the real pointer on a worktree row — hovering is the only way to
/// select one. The pointer sits a fixed inset from the row's leading edge;
/// GPUI opens the card right of the pointer, so an open card never covers the
/// spot another row is hovered at.
fn hover_worktree(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    name: &str,
) -> Point<Pixels> {
    draw(cx, window);
    app.update(cx, |app, cx| {
        let index = app.sidebar.rows.iter().position(|row| matches!(
            row, kagi::ui::sidebar::SidebarRow::Worktree { name: row_name, .. } if row_name == name
        )).expect("registered sidebar worktree");
        let first_leaf = app.sidebar.pane_ranges[2].start + 1;
        app.sidebar.scroll_handles[2]
            .scroll_to_item(index - first_leaf, gpui::ScrollStrategy::Center);
        cx.notify();
    });
    let id = format!("sidebar-worktree-{name}");
    e2e::clear_control_bounds(window.window_id(), &id);
    draw(cx, window);
    let row = bounds(window, &id).unwrap_or_else(|| panic!("{id} not drawn"));
    assert!(f32::from(row.size.height) > 0. && f32::from(row.size.width) > 4.);
    let pointer = point(row.origin.x + px(4.), row.center().y);
    cx.simulate_mouse_move(window, pointer, None, Modifiers::none());
    cx.run_until_parked();
    pointer
}

/// Let the show delay pass with the pointer resting, then prove the card is
/// the hovered row's: GPUI places it a few pixels from the pointer, it stays in
/// the window, and it floats over the panes instead of taking their height.
fn open_card(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pointer: Point<Pixels>,
    layout: ([(f32, f32); 5], Bounds<Pixels>),
) -> Bounds<Pixels> {
    cx.advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
    redraw_card(cx, window);
    let card = bounds(window, "worktree-inspection").expect("hover card not shown after the delay");
    let near = |edge: Pixels, at: Pixels| (f32::from(edge) - f32::from(at)).abs() <= 8.;
    assert!(
        (near(card.origin.x, pointer.x) || near(card.right(), pointer.x))
            && (near(card.origin.y, pointer.y) || near(card.bottom(), pointer.y)),
        "card {card:?} is not anchored at the hovered row's pointer {pointer:?}"
    );
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    assert!(
        card.origin.x >= px(0.)
            && card.origin.y >= px(0.)
            && card.right() <= viewport.width
            && card.bottom() <= viewport.height,
        "card {card:?} overflows the window {viewport:?}"
    );
    assert_eq!(
        pane_layout(cx, app, window),
        layout,
        "the hover card took space from the sidebar panes"
    );
    let heading = bounds(window, "worktree-inspection-heading").expect("card heading");
    assert!(
        heading.size.height > px(0.)
            && heading.origin.y >= card.origin.y
            && heading.bottom() <= card.bottom(),
        "worktree heading is not visible in the card"
    );
    card
}

/// Leave row and card for a neutral spot; the card closes after the hide delay.
fn leave(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let sidebar = bounds(window, "worktree-sidebar").expect("sidebar");
    let away = point(sidebar.origin.x + px(4.), sidebar.bottom() - px(4.));
    cx.simulate_mouse_move(window, away, None, Modifiers::none());
    cx.run_until_parked();
    cx.advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
    redraw_card(cx, window);
    assert!(
        !card_shown(window),
        "card outlived the pointer leaving row and card"
    );
}

/// Carry the pointer from the row into the open card — within the hide delay,
/// as a hand does — and click a control there.
fn click_in_card(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    draw(cx, window);
    let target = bounds(window, id)
        .unwrap_or_else(|| panic!("{id} not drawn in the card"))
        .center();
    cx.simulate_mouse_move(window, target, None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, target, Modifiers::none());
    cx.run_until_parked();
    redraw_card(cx, window);
    let card = bounds(window, "worktree-inspection").expect("card closed under the pointer");
    assert!(
        card.contains(&target),
        "refresh moved the card away from the pointer: {card:?} / {target:?}"
    );
}

fn observation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    path: &Path,
) -> WorktreeInspection {
    let target = cx.read(|cx| {
        app.read(cx)
            .view()
            .worktrees
            .iter()
            .find(|target| target.path == path)
            .expect("registered worktree")
            .clone()
    });
    inspect_worktree(repo, &target, &AtomicBool::new(false))
}

fn shown(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    path: &Path,
) -> (bool, Option<u64>, Option<String>) {
    cx.read(|cx| e2e::worktree_inspection::status(app.read(cx), path))
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, repo) = fixture();
    let root = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let original_lang = i18n::lang();
    // At rest nothing is hovered: no card, and nothing docked under the panes.
    redraw_card(cx, window);
    assert!(
        !card_shown(window),
        "worktree details drawn without a hover"
    );
    let layout = pane_layout(cx, &app, window);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        // Row to row with no pause between: each card must be the newly
        // hovered row's, not the one the pointer just left.
        for (index, (name, verdict)) in [
            ("pushed", Verdict::SafePushed),
            ("dirty", Verdict::Dirty),
            ("locked", Verdict::Locked),
            ("detached", Verdict::SafeMerged),
        ]
        .into_iter()
        .enumerate()
        {
            let path = root.join(name);
            let pointer = hover_worktree(cx, &app, window, name);
            if index == 0 {
                redraw_card(cx, window);
                assert!(
                    !card_shown(window),
                    "{name}: card opened before the hover delay"
                );
            }
            let card = open_card(cx, &app, window, pointer, layout);
            let (pending, bytes, reason) = shown(cx, &app, &path);
            assert!(!pending, "{name}: measurement did not settle");
            assert!(
                bytes.expect("allocation") >= 65_536,
                "ignored build output was not counted"
            );
            assert_eq!(
                reason.as_deref(),
                Some(i18n::worktree_removal_verdict_text(verdict)),
                "{name}/{lang:?}"
            );
            assert!(
                bounds(window, "worktree-inspection-verdict").is_some(),
                "reason not painted"
            );
            if name == "pushed" {
                // Wheel inside the card scrolls its body only: the card, its
                // heading and Refresh stay put, and the card stays open.
                let body = bounds(window, "worktree-inspection-body").unwrap();
                let heading = bounds(window, "worktree-inspection-heading").unwrap();
                let refresh = bounds(window, "worktree-inspection-refresh").unwrap();
                let reason_before = bounds(window, "worktree-inspection-verdict").unwrap();
                cx.simulate_mouse_move(window, body.center(), None, Modifiers::none());
                cx.run_until_parked();
                for delta in [-1000., 1000.] {
                    cx.simulate_event(
                        window,
                        gpui::ScrollWheelEvent {
                            position: body.center(),
                            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
                            touch_phase: gpui::TouchPhase::Moved,
                            ..Default::default()
                        },
                    );
                    cx.run_until_parked();
                    redraw_card(cx, window);
                    assert_eq!(
                        bounds(window, "worktree-inspection"),
                        Some(card),
                        "scrolling the card body moved or closed the card"
                    );
                    assert_eq!(
                        bounds(window, "worktree-inspection-heading"),
                        Some(heading),
                        "detail scrolling moved the identity heading"
                    );
                    assert_eq!(
                        bounds(window, "worktree-inspection-refresh"),
                        Some(refresh),
                        "detail scrolling moved Refresh"
                    );
                    let reason_now = bounds(window, "worktree-inspection-verdict");
                    if delta < 0. {
                        assert!(
                            reason_now
                                .is_none_or(|bounds| bounds.origin.y <= reason_before.origin.y),
                            "detail body scrolled the wrong way"
                        );
                    } else {
                        assert_eq!(
                            reason_now,
                            Some(reason_before),
                            "scrolling back did not restore the detail body"
                        );
                    }
                }
            }
        }
        leave(cx, window);
    }

    // Only the transport future is held. Real hover selection/refresh, request
    // revision, owner routing and rendering still execute while the old read is
    // pending.
    let pushed = root.join("pushed");
    let pointer = hover_worktree(cx, &app, window, "pushed");
    open_card(cx, &app, window, pointer, layout);
    let old = observation(cx, &app, &repo, &pushed);
    let (task, reply) = deferred(cx);
    e2e::worktree_inspection::queue(task);
    click_in_card(cx, window, "worktree-inspection-refresh");
    assert!(shown(cx, &app, &pushed).0);
    assert!(
        bounds(window, "worktree-inspection-measuring").is_some(),
        "pending spinner not drawn"
    );
    // A pointer resting inside the card holds it open past the hide delay.
    cx.advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
    redraw_card(cx, window);
    assert!(
        card_shown(window) && bounds(window, "worktree-inspection-measuring").is_some(),
        "card closed with the pointer inside it"
    );
    std::fs::write(pushed.join("target/new.bin"), vec![4; 8192]).unwrap();
    click_in_card(cx, window, "worktree-inspection-refresh");
    let newer = shown(cx, &app, &pushed);
    assert!(
        newer.1.unwrap() > old.disk_usage.as_ref().unwrap().allocated_bytes,
        "manual refresh did not observe new disk allocation"
    );
    reply.send(old);
    cx.run_until_parked();
    assert_eq!(
        shown(cx, &app, &pushed),
        newer,
        "superseded read replaced the fresh measurement"
    );
    // Leaving from inside the card closes it too.
    leave(cx, window);

    // Hovering another worktree retires the old worktree's pending observation.
    let dirty = root.join("dirty");
    let pointer = hover_worktree(cx, &app, window, "dirty");
    open_card(cx, &app, window, pointer, layout);
    let cached = shown(cx, &app, &dirty);
    std::fs::write(dirty.join("target/later.bin"), vec![2; 8192]).unwrap();
    let late = observation(cx, &app, &repo, &dirty);
    let (task, reply) = deferred(cx);
    e2e::worktree_inspection::queue(task);
    click_in_card(cx, window, "worktree-inspection-refresh");
    // Straight from dirty's card onto pushed's row: the hover alone selects.
    let pointer = hover_worktree(cx, &app, window, "pushed");
    reply.send(late);
    cx.run_until_parked();
    assert_eq!(
        shown(cx, &app, &dirty),
        cached,
        "departed selection accepted an old completion"
    );
    open_card(cx, &app, window, pointer, layout);

    // A missing fetched upstream is typed Unknown, never evidence of publication.
    git(&repo, &["update-ref", "-d", "refs/remotes/origin/pushed"]);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        click_in_card(cx, window, "worktree-inspection-refresh");
        assert_eq!(
            shown(cx, &app, &pushed).2.as_deref(),
            Some(i18n::worktree_removal_verdict_text(Verdict::Unknown(
                WorktreeUnknownReason::UpstreamUnavailable
            )))
        );
    }

    // Closing the owner drops its cache and cancellation resource. Late delivery
    // must not recreate it or populate Welcome's immutable empty state.
    let late = observation(cx, &app, &repo, &pushed);
    let (task, reply) = deferred(cx);
    e2e::worktree_inspection::queue(task);
    click_in_card(cx, window, "worktree-inspection-refresh");
    let owner = app.update(cx, |app, cx| {
        let owner = app.active_session().unwrap();
        app.close_tab(0, cx);
        owner
    });
    reply.send(late);
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(!app.read(cx).ui.contains_key(&owner));
        assert_eq!(
            e2e::worktree_inspection::status(app.read(cx), &pushed),
            (false, None, None)
        );
    });
    redraw_card(cx, window);
    assert!(!card_shown(window), "card outlived its closed tab");

    // #779: one accepted observation is not tab-wide coverage. A request
    // retired by leaving the tab left every worktree it never reached
    // unmeasured, and returning refused to schedule them because the cache was
    // merely non-empty.
    let other = root.join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("file.txt"), "other\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "other"]);

    // A reopened tab starts with an empty cache, and its automatic sweep is
    // held on the production transport seam so nothing is measured behind the
    // test's back.
    let (held, retired) = deferred(cx);
    e2e::worktree_inspection::queue(held);
    assert!(app.update(cx, |app, cx| app.open_repository(repo.clone(), cx)));
    cx.run_until_parked();
    let repo_tab = app.update(cx, |app, _| app.active_tab);
    assert_eq!(
        shown(cx, &app, &pushed),
        (true, None, None),
        "reopened tab did not start an automatic sweep"
    );

    // Hovering retires that sweep and measures exactly one worktree — the
    // partial cache a tab switch leaves behind once a first result has landed.
    let (first, deliver) = deferred(cx);
    e2e::worktree_inspection::queue(first);
    hover_worktree(cx, &app, window, "pushed");
    retired.send(observation(cx, &app, &repo, &pushed));
    cx.run_until_parked();
    deliver.send(observation(cx, &app, &repo, &pushed));
    cx.run_until_parked();
    let measured = shown(cx, &app, &pushed);
    assert!(
        !measured.0 && measured.1.is_some(),
        "the selected worktree was not measured"
    );
    for name in ["dirty", "locked", "detached"] {
        assert_eq!(
            shown(cx, &app, &root.join(name)),
            (false, None, None),
            "{name}: the retired sweep measured more than the selection"
        );
    }

    // Change the repository before leaving through the real tab lifecycle:
    // returning must discover the added worktree without re-walking cached ones.
    std::fs::write(pushed.join("target/while-away.bin"), vec![9; 131_072]).unwrap();
    let resumed = root.join("resumed");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "resumed",
            resumed.to_str().unwrap(),
            "main",
        ],
    );
    assert!(app.update(cx, |app, cx| app.open_repository(other.clone(), cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(repo_tab, cx));
    cx.run_until_parked();
    draw(cx, window);
    for name in ["dirty", "locked", "detached", "resumed"] {
        let (pending, bytes, _) = shown(cx, &app, &root.join(name));
        assert!(
            !pending && bytes.is_some(),
            "{name}: returning to the tab never measured it"
        );
    }
    assert_eq!(
        shown(cx, &app, &pushed).1,
        measured.1,
        "the completed observation was discarded or re-walked on return"
    );
    assert!(
        shown(cx, &app, &pushed).2.is_some(),
        "the completed observation lost its verdict"
    );

    i18n::set_lang(original_lang);
    eprintln!("[gui-e2e] PASS worktree_inspection EN/JA hover card (delay, row anchor, panes intact, row→row, row→card Refresh, leave closes), ignored allocation, refresh, stale/hover/close rejection, partial-cache resume on return");
    unmount(cx, app, window);
}
