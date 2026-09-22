//! Real worktree observations through native selection/refresh and stale delivery.
use crate::{
    evidence_support::deferred,
    macos::{git, mount, unmount},
};
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
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
};

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

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    draw(cx, window);
    let bounds =
        e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} not drawn"));
    assert!(f32::from(bounds.size.height) > 0. && f32::from(bounds.size.width) > 0.);
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw(cx, window);
}

fn select_worktree(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    name: &str,
) {
    draw(cx, window);
    // Selection opens a detail panel and shrinks the virtual viewport. Bring
    // the real row into view before clicking it, without bypassing selection.
    app.update(cx, |app, cx| {
        let index = app.sidebar.rows.iter().position(|row| matches!(
            row, kagi::ui::sidebar::SidebarRow::Worktree { name: row_name, .. } if row_name == name
        )).expect("registered sidebar worktree");
        app.sidebar
            .scroll_handle
            .scroll_to_item(index, gpui::ScrollStrategy::Center);
        cx.notify();
    });
    click(cx, window, &format!("sidebar-worktree-{name}"));
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
    let (fixture, repo) = fixture();
    let root = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let original_lang = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for (name, verdict) in [
            ("pushed", Verdict::SafePushed),
            ("dirty", Verdict::Dirty),
            ("locked", Verdict::Locked),
            ("detached", Verdict::SafeMerged),
        ] {
            let path = root.join(name);
            select_worktree(cx, &app, window, name);
            let sidebar = e2e::control_bounds(window.window_id(), "worktree-sidebar").unwrap();
            let panel = e2e::control_bounds(window.window_id(), "worktree-inspection").unwrap();
            let heading =
                e2e::control_bounds(window.window_id(), "worktree-inspection-heading").unwrap();
            assert!(
                f32::from(panel.size.height) <= f32::from(sidebar.size.height) * 0.4 + 1.,
                "detail panel displaced the navigator: {panel:?} / {sidebar:?}"
            );
            assert!(
                heading.size.height > gpui::px(0.)
                    && heading.origin.y >= panel.origin.y
                    && heading.bottom() <= panel.bottom(),
                "worktree heading is not visible"
            );
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
                e2e::control_bounds(window.window_id(), "worktree-inspection-verdict").is_some(),
                "reason not painted"
            );
            if name == "pushed" {
                let body =
                    e2e::control_bounds(window.window_id(), "worktree-inspection-body").unwrap();
                let reason_before =
                    e2e::control_bounds(window.window_id(), "worktree-inspection-verdict").unwrap();
                for delta in [-1000., 1000.] {
                    e2e::clear_control_bounds(window.window_id(), "worktree-inspection-verdict");
                    cx.simulate_event(
                        window,
                        gpui::ScrollWheelEvent {
                            position: body.center(),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(0.),
                                gpui::px(delta),
                            )),
                            touch_phase: gpui::TouchPhase::Moved,
                            ..Default::default()
                        },
                    );
                    draw(cx, window);
                    assert_eq!(
                        e2e::control_bounds(window.window_id(), "worktree-inspection-heading"),
                        Some(heading),
                        "detail scrolling moved the identity heading"
                    );
                    if delta < 0. {
                        assert!(
                            e2e::control_bounds(window.window_id(), "worktree-inspection-verdict")
                                .is_none_or(|bounds| bounds.origin.y < reason_before.origin.y),
                            "detail body did not scroll independently"
                        );
                    }
                }
            }
        }
    }

    // Only the transport future is held. Real refresh/selection, request revision,
    // owner routing and rendering still execute while the old read is pending.
    let pushed = root.join("pushed");
    select_worktree(cx, &app, window, "pushed");
    let old = observation(cx, &app, &repo, &pushed);
    let (task, reply) = deferred(cx);
    e2e::worktree_inspection::queue(task);
    click(cx, window, "worktree-inspection-refresh");
    assert!(shown(cx, &app, &pushed).0);
    assert!(
        e2e::control_bounds(window.window_id(), "worktree-inspection-measuring").is_some(),
        "pending spinner not drawn"
    );
    std::fs::write(pushed.join("target/new.bin"), vec![4; 8192]).unwrap();
    click(cx, window, "worktree-inspection-refresh");
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

    // Changing selection retires the old worktree's pending observation.
    let dirty = root.join("dirty");
    select_worktree(cx, &app, window, "dirty");
    let cached = shown(cx, &app, &dirty);
    std::fs::write(dirty.join("target/later.bin"), vec![2; 8192]).unwrap();
    let late = observation(cx, &app, &repo, &dirty);
    let (task, reply) = deferred(cx);
    e2e::worktree_inspection::queue(task);
    click(cx, window, "worktree-inspection-refresh");
    select_worktree(cx, &app, window, "pushed");
    reply.send(late);
    cx.run_until_parked();
    assert_eq!(
        shown(cx, &app, &dirty),
        cached,
        "departed selection accepted an old completion"
    );

    // A missing fetched upstream is typed Unknown, never evidence of publication.
    git(&repo, &["update-ref", "-d", "refs/remotes/origin/pushed"]);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        click(cx, window, "worktree-inspection-refresh");
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
    click(cx, window, "worktree-inspection-refresh");
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

    // Selecting retires that sweep and measures exactly one worktree — the
    // partial cache a tab switch leaves behind once a first result has landed.
    let (first, deliver) = deferred(cx);
    e2e::worktree_inspection::queue(first);
    select_worktree(cx, &app, window, "pushed");
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
    eprintln!("[gui-e2e] PASS worktree_inspection EN/JA, ignored allocation, refresh, stale/selection/close rejection, partial-cache resume on return");
    unmount(cx, app, window);
}
