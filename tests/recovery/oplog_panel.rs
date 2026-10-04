//! #334 (ADR-0214): the Operation Log panel reads — actor and worktree badges
//! on each row; a selected row shows the ref moves it recorded (slice 2a), or,
//! for an entry without a record, reflog lines estimated by its time window
//! with a shared second marked ambiguous (slice 1). Nothing is written.
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::oplog_panel::{entry_worktree, ReflogDetail};
use kagi::ui::{e2e, i18n, KagiApp};
use kagi_domain::oplog_reflog::Attribution;
use kagi_domain::ref_moves::RefMove;
use kagi_git::oplog::{append_oplog, read_oplog_tail, Actor, OpLogEntry, OpOutcome};
use kagi_git::{Backend, CommitId, Operation, StateSummary};

use crate::macos::{build_fixture, git, mount, open_offscreen, repo_fingerprint, unmount};
use crate::recovery_operations::{press_key, wait_idle};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git_command, git_output};

/// Sleep into the next wall-clock second, so two writes never share one.
fn next_second() {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    std::thread::sleep(Duration::from_nanos(
        1_000_000_000 - u64::from(now.subsec_nanos()) + 20_000_000,
    ));
}

fn run(backend: &mut Backend, actor: Actor, op: Operation) {
    backend.set_actor(actor);
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend.run(&op, &plan).unwrap();
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

/// Panel row indices of `repo`'s entries, newest first.
fn rows_of(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) -> Vec<usize> {
    let repo = repo.to_string_lossy().into_owned();
    cx.read(|cx| {
        let panel = app.read(cx).op_log.clone().expect("op_log");
        let panel = panel.read(cx);
        panel
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| entry_worktree(e) == repo)
            .map(|(i, _)| i)
            .collect()
    })
}

/// Click row `i` for real (scrolled into view first) and require the click
/// to select it.
fn click_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    i: usize,
) {
    let panel = cx.read(|cx| app.read(cx).op_log.clone().expect("op_log"));
    // What a person does for a row below the fold: scroll it into view.
    cx.read(|cx| panel.read(cx).scroll_handle().scroll_to_reveal_item(i));
    paint(cx, window);
    let bounds = cx
        .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(i))
        .unwrap_or_else(|| panic!("op-log row {i} was not laid out"));
    let mut point = bounds.origin;
    point.x += gpui::px(40.);
    point.y += gpui::px(8.);
    cx.simulate_click(window, point, gpui::Modifiers::none());
    cx.run_until_parked();
    let expanded = cx.read(|cx| panel.read(cx).expanded());
    assert_eq!(
        expanded,
        Some(i),
        "the click at {point:?} (row {i} at {bounds:?}) must select row {i}"
    );
}

/// Select a row **without** recorded moves and wait for its estimated
/// (time-window) reflog lines.
fn select_estimated(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    i: usize,
) -> Vec<(String, String, Attribution)> {
    click_row(cx, app, window, i);
    let panel = cx.read(|cx| app.read(cx).op_log.clone().expect("op_log"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let lines = cx.read(|cx| match panel.read(cx).reflog_detail() {
            Some(ReflogDetail::Loaded { lines, .. }) => Some(
                lines
                    .iter()
                    .map(|(l, a)| (l.refname.clone(), l.message.clone(), *a))
                    .collect::<Vec<_>>(),
            ),
            Some(ReflogDetail::Unavailable(e)) => panic!("reflog unavailable: {e}"),
            _ => None,
        });
        if let Some(lines) = lines {
            paint(cx, window);
            return lines;
        }
        assert!(Instant::now() < deadline, "row {i}'s reflog never loaded");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Select a row **with** recorded moves: the record is painted, the estimate
/// is not, and no reflog read was started. Returns the recorded moves.
fn select_recorded(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    i: usize,
) -> Vec<RefMove> {
    for probe in [
        format!("oplog-recorded-{i}"),
        format!("oplog-reflog-{i}-0-within"),
    ] {
        e2e::clear_control_bounds(window.window_id(), &probe);
    }
    click_row(cx, app, window, i);
    paint(cx, window);
    assert!(
        painted(window, &format!("oplog-recorded-{i}")),
        "row {i}: the record is shown"
    );
    assert!(
        !painted(window, &format!("oplog-reflog-{i}-0-within")),
        "row {i}: a recorded row shows no estimate"
    );
    cx.read(|cx| {
        let panel = app.read(cx).op_log.clone().expect("op_log");
        let panel = panel.read(cx);
        assert!(
            panel.reflog_detail().is_none(),
            "no estimate read for a record"
        );
        panel.entries()[i].ref_moves.clone().expect("recorded")
    })
}

fn painted(window: AnyWindowHandle, name: &str) -> bool {
    e2e::control_bounds(window.window_id(), name).is_some()
}

pub fn scenario_oplog_actor_reflog(cx: &mut VisualTestAppContext) {
    // ── three recorded writes, three actors, three seconds ───────────────
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));
    let mut backend = Backend::open(&repo).unwrap();
    next_second();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: "oplog-a".into(),
            at: head.clone(),
        },
    );
    next_second();
    run(
        &mut backend,
        Actor::Mcp,
        Operation::Checkout {
            branch: "oplog-a".into(),
        },
    );
    next_second();
    run(
        &mut backend,
        Actor::Cli,
        Operation::CreateBranch {
            name: "oplog-b".into(),
            at: head,
        },
    );

    // ── a second worktree whose two operations share one second ──────────
    let shared_root = tempfile::tempdir().unwrap();
    let shared = shared_root.path().join("shared-second");
    std::fs::create_dir(&shared).unwrap();
    git(&shared, &["init", "-q", "-b", "main"]);
    std::fs::write(shared.join("f.txt"), "f\n").unwrap();
    git(&shared, &["add", "."]);
    git(&shared, &["commit", "-qm", "base"]);
    let shared = shared.canonicalize().unwrap();
    let second: i64 = 1_700_000_000;
    let out = git_command(&shared)
        .args([
            "update-ref",
            "-m",
            "shared: moved",
            "refs/heads/side",
            "HEAD",
        ])
        .env("GIT_COMMITTER_DATE", format!("@{second} +0000"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for op in ["first-in-second", "second-in-second"] {
        let state = StateSummary {
            head: "branch: main".into(),
            dirty: "clean".into(),
        };
        let mut entry = OpLogEntry::new(
            op,
            shared.display().to_string(),
            state.clone(),
            OpOutcome::Success { after: state },
        )
        .with_worktree(Some(shared.display().to_string()));
        entry.timestamp = second;
        append_oplog(&entry).unwrap();
    }

    let before = (repo_fingerprint(&repo), read_oplog_tail(500).len());
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        cx.notify();
    });
    paint(cx, window);

    // ── one row per operation, with its actor and worktree ───────────────
    let rows = rows_of(cx, &app, &repo);
    assert_eq!(rows.len(), 3, "one row per recorded operation: {rows:?}");
    let name = repo.file_name().unwrap().to_string_lossy().into_owned();
    for (i, actor) in rows.iter().zip(["cli", "mcp", "human"]) {
        assert!(
            painted(window, &format!("oplog-actor-{i}-{actor}")),
            "row {i} shows the {actor} badge"
        );
        assert!(
            painted(window, &format!("oplog-worktree-{i}-{name}")),
            "row {i} shows the worktree badge {name}"
        );
    }

    // ── recorded rows show their record, not an estimate (#334 slice 2a) ─
    let checkout = rows[1];
    let moves = select_recorded(cx, &app, window, checkout);
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert_eq!(moves[0].refname, "HEAD");
    assert_eq!(moves[0].old_symbolic.as_deref(), Some("refs/heads/main"));
    assert_eq!(moves[0].new_symbolic.as_deref(), Some("refs/heads/oplog-a"));
    assert!(painted(window, &format!("oplog-refmove-{checkout}-0-HEAD")));

    let created = rows[2];
    let moves = select_recorded(cx, &app, window, created);
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert_eq!(moves[0].refname, "refs/heads/oplog-a");
    assert_eq!(moves[0].old, None, "created");
    assert!(painted(
        window,
        &format!("oplog-refmove-{created}-0-refs/heads/oplog-a")
    ));

    // ── a shared second is shown, marked, never attributed ───────────────
    let shared_rows = rows_of(cx, &app, &shared);
    assert_eq!(shared_rows.len(), 2);
    let lines = select_estimated(cx, &app, window, shared_rows[0]);
    assert_eq!(
        lines,
        vec![(
            "refs/heads/side".to_string(),
            "shared: moved".to_string(),
            Attribution::Ambiguous
        )]
    );
    assert!(painted(
        window,
        &format!("oplog-reflog-{}-0-ambiguous", shared_rows[0])
    ));
    assert!(
        !painted(window, &format!("oplog-recorded-{}", shared_rows[0])),
        "an entry without a record is shown as an estimate only"
    );
    // ── reading wrote nothing and logged nothing ─────────────────────────
    assert_eq!(
        (repo_fingerprint(&repo), read_oplog_tail(500).len()),
        before,
        "the panel only reads"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS oplog_actor_reflog: 3 rows with actor/worktree badges; recorded rows show their ref moves, not an estimate; an unrecorded row's shared second is marked ambiguous");
}

/// Click a painted control (a probe registered by `measure_inside`).
fn click_probe(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    e2e::clear_control_bounds(window.window_id(), name);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} was not painted"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn branch_names(repo: &Path) -> String {
    git_output(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
}

/// The card the open entry point leads to, read back from the app.
fn restore_card(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> kagi::ui::modals::oplog_restore::OplogRestoreModal {
    cx.read(|cx| app.read(cx).oplog_restore_modal().cloned())
        .expect("the op-revert / restore card is open")
}

/// Confirm the open card twice (destructive: arm, then run) and wait for it.
fn confirm_twice(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let before = restore_card(cx, app);
    assert!(!before.confirm_armed);
    let ref_count = before.restores.len();
    press_key(cx, app, window, "enter");
    cx.run_until_parked();
    let armed = restore_card(cx, app);
    assert!(armed.confirm_armed, "the first Enter only arms");
    assert_eq!(armed.restores.len(), ref_count);
    assert!(armed.confirm_label().contains(&ref_count.to_string()));
    assert_ne!(before.confirm_label(), armed.confirm_label());
    press_key(cx, app, window, "enter");
    wait_idle(cx, app);
    assert!(cx.read(|cx| app.read(cx).oplog_restore_modal().is_none()));
}

/// #334 slice 2b-2: a selected row's real buttons open the card; it lists
/// every branch's reverse action and what is not restored; two confirms
/// restore the branches; the restore's own row can be reverted the same way.
/// A row without recorded moves has both buttons disabled. #334 slice 2c: the
/// card draws the graph after (where main goes back to, how many commits
/// leave every branch — checked against git after confirming), or says the
/// preview is unavailable when the target is not loaded.
pub fn scenario_oplog_restore_card(cx: &mut VisualTestAppContext) {
    use kagi_domain::plan_note::{OplogRestoreNote, PlanNote};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();

    let fixture = build_fixture();
    // Move the fixture under a long worktree path: the warning must leave the
    // reason legible while exposing the complete path via Copy all and AX.
    let long_root = tempfile::tempdir().unwrap();
    let repo = long_root.path().join("a-long-worktree-path-for-restore-warnings-with-several-nested-components-and-a-final-directory");
    std::fs::rename(fixture.path(), &repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    let head = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));
    let mut backend = Backend::open(&repo).unwrap();
    for name in ["keep", "drop1", "drop2"] {
        run(
            &mut backend,
            Actor::Human,
            Operation::CreateBranch {
                name: name.into(),
                at: head.clone(),
            },
        );
    }
    // #334 slice 2c: a recorded commit on main, so restoring to the first
    // creation moves main back one commit and leaves that commit on no branch.
    let main_before = head.0.clone();
    std::fs::write(repo.join("README.md"), "# fixture\nmoved by restore\n").unwrap();
    git(&repo, &["add", "README.md"]);
    run(
        &mut backend,
        Actor::Human,
        Operation::Commit {
            message: "on main after the branches".into(),
        },
    );
    let main_after = git_output(&repo, &["rev-parse", "main"]);
    let all_branches = branch_names(&repo);
    let on_branches = |repo: &Path| -> usize {
        git_output(repo, &["rev-list", "--count", "--branches"])
            .parse()
            .unwrap()
    };

    // An entry of another (existing) repository without recorded moves. It
    // must be a repository: an entry whose worktree cannot be opened is of
    // unknown origin and blocks a restore across it (#878 review).
    let elsewhere_root = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere_root.path().canonicalize().unwrap();
    git(&elsewhere, &["init", "-q", "-b", "main"]);
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let unrecorded = OpLogEntry::new(
        "unrecorded",
        elsewhere.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    )
    .with_worktree(Some(elsewhere.display().to_string()));
    append_oplog(&unrecorded).unwrap();

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        app.bottom_panel_height = 600.;
        cx.notify();
    });
    paint(cx, window);
    // A blocked restore with no canonical rows still shows its blockers and
    // cannot expose a Confirm button or AX confirm action.
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(Operation::OpRevert { entry_id: u64::MAX }, cx)
    });
    let blocked = restore_card(cx, &app);
    assert!(blocked.restores.is_empty());
    assert!(!blocked.plan.blockers.is_empty());
    paint(cx, window);
    assert!(!painted(window, "plan-confirm"));
    let actions = kagi::ui::dialog_a11y::recorded_dialog("plan-card")
        .expect("blocked dialog")
        .actions;
    assert_eq!(actions.len(), 1, "blocked dialog offers only Cancel");
    assert_eq!(actions[0].1, i18n::Msg::PlanCancel.t());
    press_key(cx, &app, window, "escape");

    // ── no record: both buttons disabled ─────────────────────────────────
    let other = rows_of(cx, &app, &elsewhere)[0];
    click_row(cx, &app, window, other);
    paint(cx, window);
    assert!(painted(window, &format!("oplog-revert-{other}-disabled")));
    assert!(painted(window, &format!("oplog-restore-{other}-disabled")));

    // ── restore to the first creation: the card, then two confirms ──────
    let rows = rows_of(cx, &app, &repo);
    assert_eq!(rows.len(), 4, "{rows:?}");
    let keep = rows[3];
    let keep_id = cx.read(|cx| {
        let panel = app.read(cx).op_log.clone().unwrap();
        panel.read(cx).entries()[keep].id
    });
    click_row(cx, &app, window, keep);
    click_probe(cx, window, &format!("oplog-restore-{keep}-enabled"));
    let card = restore_card(cx, &app);
    assert_eq!(card.op, Operation::RestoreToPoint { entry_id: keep_id });
    assert!(card.plan.blockers.is_empty(), "{:?}", card.plan.blockers);
    assert_eq!(card.restores.len(), 3);
    for branch in ["refs/heads/drop1", "refs/heads/drop2"] {
        assert!(
            card.restores
                .iter()
                .any(|r| r.refname == branch && r.restore_to.is_none()),
            "delete {branch}: {:?}",
            card.restores
        );
    }
    assert!(card
        .restores
        .iter()
        .any(|r| r.refname == "refs/heads/main"
            && r.restore_to.as_deref() == Some(main_before.as_str())));
    assert!(card
        .plan
        .warnings
        .contains(&PlanNote::OplogRestore(OplogRestoreNote::RefsOnly)));
    let check_checked_out = |cx: &mut VisualTestAppContext, dirty: bool| {
        let card = restore_card(cx, &app);
        let (index, path) = card
            .plan
            .warnings
            .iter()
            .enumerate()
            .find_map(|(index, note)| match (note, dirty) {
                (
                    PlanNote::OplogRestore(OplogRestoreNote::CheckedOutDirty { branch, path }),
                    true,
                )
                | (
                    PlanNote::OplogRestore(OplogRestoreNote::MovesCheckedOutBranch {
                        branch,
                        path,
                    }),
                    false,
                ) if branch == "main" => Some((index, path)),
                _ => None,
            })
            .expect("moving checked-out main warns for its worktree");
        assert_eq!(std::fs::canonicalize(path).unwrap(), repo);
        let id = format!("restore-warning-{index}");
        for language in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(language);
            paint(cx, window);
            let reason = i18n::oplog_panel::restore_checked_out("main", dirty);
            let note = kagi::ui::dialog_a11y::recorded_note(&id).expect("painted warning");
            assert_eq!(note, (gpui::Role::Note, format!("{reason} · {path}")));
            let row = e2e::control_bounds(window.window_id(), &id).expect("warning row");
            let path_bounds =
                e2e::control_bounds(window.window_id(), &format!("restore-warning-path-{index}"))
                    .expect("visible worktree path");
            assert!(path_bounds.size.width > px(0.));
            assert!(
                path_bounds.left() >= row.left() && path_bounds.right() <= row.right() + px(1.),
                "long path stays within the warning row: {path_bounds:?} vs {row:?}"
            );
            click_probe(cx, window, "plan-card-copy");
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .expect("copied warning");
            assert!(copied.contains(&format!("{reason} · {path}")), "{copied}");
        }
    };
    check_checked_out(cx, false);
    press_key(cx, &app, window, "escape");
    let dirty_file = repo.join("restore-warning-untracked.txt");
    std::fs::write(&dirty_file, "uncommitted\n").unwrap();
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(Operation::RestoreToPoint { entry_id: keep_id }, cx);
    });
    check_checked_out(cx, true);
    std::fs::remove_file(dirty_file).unwrap();
    press_key(cx, &app, window, "escape");
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(Operation::RestoreToPoint { entry_id: keep_id }, cx);
    });
    i18n::set_lang(original_language);
    paint(cx, window);
    assert!(
        painted(window, "plan-confirm"),
        "the card is drawn with its confirm"
    );
    assert_eq!(branch_names(&repo), all_branches, "planning moved nothing");

    // ── the graph after (#334 slice 2c) ──────────────────────────────────
    let preview = card
        .preview
        .clone()
        .expect("a restore that moves refs has a preview");
    let kagi_domain::restore_preview::RestorePreview::Graph {
        rows: prow,
        removed,
        ..
    } = &preview.graph
    else {
        panic!("every target is loaded: {:?}", preview.graph)
    };
    assert!(prow.len() <= kagi_domain::restore_preview::PREVIEW_MAX_ROWS);
    assert!(prow.len() < 6, "short preview fixture: {} rows", prow.len());
    let list = e2e::control_bounds(window.window_id(), "restore-preview-rows")
        .expect("short preview viewport");
    let row_h = e2e::control_bounds(window.window_id(), "restore-preview-row-0")
        .expect("first preview row")
        .size
        .height;
    assert!(
        (list.size.height - row_h * prow.len() as f32).abs() < gpui::px(1.),
        "viewport should fit its {} rows: {list:?}, row height {row_h:?}",
        prow.len()
    );
    assert_eq!(*removed, 1, "the commit made on main after the branches");
    let main_row = prow
        .iter()
        .find(|r| r.moved_here == vec!["main".to_string()])
        .expect("main is drawn where it goes back to");
    assert_eq!(main_row.id.0, main_before);
    assert!(
        prow.iter().any(|r| r.id.0 == main_after && r.off_branch),
        "the commit leaving every branch remains as a ghost"
    );
    assert!(!main_row.off_branch, "restored tip is reachable");
    assert!(painted(window, "restore-preview"));
    assert!(painted(window, "restore-preview-removed-1"));
    assert!(painted(
        window,
        &format!("restore-preview-moved-main-{}", main_row.id.short())
    ));
    assert!(painted(
        window,
        &format!("restore-preview-ghost-{}", &main_after[..8])
    ));
    assert!(painted(window, "restore-ref-0") && painted(window, "restore-ref-1"));
    assert!(!painted(window, "plan-state-current"));
    assert!(!painted(window, "plan-state-predicted"));
    // A compact native window must still show destructive targets and Confirm.
    let small_state = e2e::app_state(&repo).expect("compact app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let out = captured.clone();
    let small_window = open_offscreen(cx, size(px(1200.), px(300.)), move |window, cx| {
        e2e::mount_root(small_state, window, cx, &out)
    });
    let small_app = captured.borrow().clone().expect("compact Kagi");
    let small_window: AnyWindowHandle = small_window.into();
    small_app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(Operation::RestoreToPoint { entry_id: keep_id }, cx);
    });
    paint(cx, small_window);
    let id = small_window.window_id();
    let card_bounds = e2e::control_bounds(id, "modal-card").expect("compact card");
    let refs_bounds = e2e::control_bounds(id, "restore-refs").expect("compact refs");
    let confirm_bounds = e2e::control_bounds(id, "plan-confirm").expect("compact confirm");
    assert!(card_bounds.bottom() <= px(300.));
    assert!(refs_bounds.size.height >= px(3. * 31. - 1.));
    assert!(refs_bounds.top() >= card_bounds.top());
    assert!(refs_bounds.bottom() <= card_bounds.bottom());
    for n in 0..3 {
        let row = e2e::control_bounds(id, &format!("restore-ref-{n}")).expect("target ref row");
        assert!(row.top() >= refs_bounds.top() && row.bottom() <= refs_bounds.bottom() + px(1.));
        assert!(row.top() >= card_bounds.top() && row.bottom() <= card_bounds.bottom());
    }
    assert!(confirm_bounds.bottom() <= card_bounds.bottom() + px(1.));
    unmount(cx, small_app, small_window);

    let count_before = on_branches(&repo);

    confirm_twice(cx, &app, window);
    assert_eq!(
        branch_names(&repo),
        "keep\nmain",
        "drop1 / drop2 removed, keep kept"
    );
    let restore = read_oplog_tail(1).pop().unwrap();
    assert_eq!(restore.op, "restore-to-point");
    assert_eq!(git_output(&repo, &["rev-parse", "main"]), main_before);
    assert_eq!(
        count_before - on_branches(&repo),
        *removed,
        "the preview's removed rows are exactly the commits that left every branch"
    );

    // ── the restore's own row: revert it from the panel ──────────────────
    let newest = cx.read(|cx| {
        let panel = app.read(cx).op_log.clone().unwrap();
        let panel = panel.read(cx);
        panel
            .entries()
            .iter()
            .position(|e| e.id == restore.id)
            .expect("the restore is a row")
    });
    click_row(cx, &app, window, newest);
    click_probe(cx, window, &format!("oplog-revert-{newest}-enabled"));
    assert_eq!(
        restore_card(cx, &app).op,
        Operation::OpRevert {
            entry_id: restore.id
        }
    );
    // main goes back to the commit the restore took off every branch: the
    // reloaded tab no longer holds it, so the card says so instead of guessing.
    assert_eq!(
        restore_card(cx, &app).preview.map(|p| p.graph.clone()),
        Some(kagi_domain::restore_preview::RestorePreview::NotLoaded {
            refname: "refs/heads/main".into(),
            oid: main_after.clone(),
        })
    );
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        paint(cx, window);
        assert!(painted(window, "restore-preview-unavailable"));
        assert!(!painted(window, "plan-recovery-scroll"));
        click_probe(cx, window, "plan-card-copy");
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert!(copied.contains("refs/heads/main"), "{copied}");
        assert!(copied.contains("git update-ref --stdin"), "{copied}");
    }
    i18n::set_lang(original_language);
    confirm_twice(cx, &app, window);
    assert_eq!(branch_names(&repo), all_branches, "the restore is undone");
    assert_eq!(git_output(&repo, &["rev-parse", "main"]), main_after);
    assert_eq!(read_oplog_tail(1).pop().unwrap().op, "op-revert");

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS oplog_restore_card: unrecorded row disabled; card lists reverse actions, what stays and the graph after (main's target row, 1 commit off every branch = git's count); two confirms restore; the revert card says its preview is unavailable; the restore is reverted from its own row");
}

fn check_blocked_restore(cx: &mut VisualTestAppContext) {
    use kagi_domain::plan_note::{OplogRestoreNote, PlanNote};

    let blocked_fixture = build_fixture();
    let blocked_repo = blocked_fixture.path().canonicalize().unwrap();
    let head = CommitId(git_output(&blocked_repo, &["rev-parse", "HEAD"]));
    let mut backend = Backend::open(&blocked_repo).unwrap();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: "checked-out".into(),
            at: head,
        },
    );
    let create_id = read_oplog_tail(1).pop().unwrap().id;
    git(&blocked_repo, &["checkout", "-q", "checked-out"]);
    let before = (repo_fingerprint(&blocked_repo), read_oplog_tail(500).len());
    let (app, window) = mount(cx, &blocked_repo);
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(
            Operation::OpRevert {
                entry_id: create_id,
            },
            cx,
        )
    });
    let blocked = restore_card(cx, &app);
    assert!(!blocked.restores.is_empty());
    assert!(blocked.plan.blockers.iter().any(|note| matches!(
        note,
        PlanNote::OplogRestore(OplogRestoreNote::DeletesCheckedOutBranch { branch, .. })
            if branch == "checked-out"
    )));
    assert!(
        blocked.plan.equivalent_command.is_some(),
        "backend carries a command even for blocked plans"
    );
    paint(cx, window);
    assert!(!painted(window, "plan-confirm"));
    assert!(!painted(window, "plan-equivalent-command"));
    assert!(!painted(window, "plan-equivalent-command-copy"));
    click_probe(cx, window, "plan-card-copy");
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap();
    assert!(copied.contains("refs/heads/checked-out"), "{copied}");
    assert!(!copied.contains("git update-ref --stdin"), "{copied}");
    press_key(cx, &app, window, "enter");
    assert_eq!(
        repo_fingerprint(&blocked_repo),
        before.0,
        "blocked Enter does not move refs"
    );
    assert_eq!(read_oplog_tail(500).len(), before.1 + 1);
    assert!(
        matches!(
            read_oplog_tail(1).pop().unwrap().outcome,
            OpOutcome::Refused { .. }
        ),
        "blocked Enter records a refusal rather than executing the restore"
    );
    unmount(cx, app, window);
}

fn check_long_ref(cx: &mut VisualTestAppContext) {
    let long_fixture = build_fixture();
    let long_repo = long_fixture.path().canonicalize().unwrap();
    let long_name = format!("feature/{}", "very-long-ref-name-".repeat(12));
    let old_tip = CommitId(git_output(&long_repo, &["rev-parse", "HEAD"]));
    let mut backend = Backend::open(&long_repo).unwrap();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: long_name.clone(),
            at: old_tip.clone(),
        },
    );
    let point_id = read_oplog_tail(1).pop().unwrap().id;
    git(&long_repo, &["checkout", "-q", &long_name]);
    std::fs::write(long_repo.join("README.md"), "# fixture\nlong ref\n").unwrap();
    git(&long_repo, &["add", "README.md"]);
    run(
        &mut backend,
        Actor::Human,
        Operation::Commit {
            message: "move the long ref".into(),
        },
    );
    git(&long_repo, &["checkout", "-q", "main"]);
    let before = (repo_fingerprint(&long_repo), read_oplog_tail(500).len());
    let (app, window) = mount(cx, &long_repo);
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(Operation::RestoreToPoint { entry_id: point_id }, cx)
    });
    let card = restore_card(cx, &app);
    assert!(card.plan.blockers.is_empty(), "{:?}", card.plan.blockers);
    let (index, row) = card
        .restores
        .iter()
        .enumerate()
        .find(|(_, row)| row.refname == format!("refs/heads/{long_name}"))
        .expect("long branch moves back to its initial commit");
    assert!(row.expect.is_some() && row.restore_to.is_some());
    assert_eq!(row.restore_to.as_deref(), Some(old_tip.0.as_str()));
    paint(cx, window);
    let id = window.window_id();
    let card_bounds = e2e::control_bounds(id, "modal-card").expect("restore card");
    let row_bounds =
        e2e::control_bounds(id, &format!("restore-ref-{index}")).expect("long ref row");
    let name_bounds =
        e2e::control_bounds(id, &format!("restore-ref-name-{index}")).expect("bounded ref name");
    assert!(name_bounds.size.width <= row_bounds.size.width * 0.45 + px(1.));
    for side in ["expected", "destination"] {
        let oid = e2e::control_bounds(id, &format!("restore-ref-{side}-{index}"))
            .unwrap_or_else(|| panic!("{side} OID not painted"));
        assert!(oid.size.width > px(0.), "{side} OID must be visible");
        assert!(
            oid.left() >= row_bounds.left() && oid.right() <= row_bounds.right() + px(1.),
            "{side} OID outside row: {oid:?} vs {row_bounds:?}"
        );
        assert!(
            oid.right() <= card_bounds.right(),
            "{side} OID outside card"
        );
    }
    let note = kagi::ui::dialog_a11y::recorded_note(&format!("restore-ref-name-{index}"))
        .expect("full ref name in accessibility tree");
    assert_eq!(note.1, format!("refs/heads/{long_name}"));
    click_probe(cx, window, "plan-card-copy");
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap();
    assert!(
        copied.contains(&format!("refs/heads/{long_name}")),
        "{copied}"
    );
    assert_eq!(
        (repo_fingerprint(&long_repo), read_oplog_tail(500).len()),
        before,
        "inspecting and copying a long ref must not mutate the repo"
    );
    unmount(cx, app, window);
}

/// #993 review: a blocked restore must not offer an unguarded CLI escape
/// hatch, and a long ref must leave both compare OIDs on the card.
pub fn scenario_oplog_restore_guarded_rows(cx: &mut VisualTestAppContext) {
    check_blocked_restore(cx);
    check_long_ref(cx);
    eprintln!("[gui-e2e] PASS oplog_restore_guarded_rows: blocked command hidden from card and Copy all; long ref keeps both OIDs inside card and full AX name; neither path mutates");
}

/// #887: a tag ref is restorable even though its raw OID is not necessarily
/// a commit. The card must avoid predicting disappearing graph rows.
pub fn scenario_oplog_restore_tag_preview(cx: &mut VisualTestAppContext) {
    use kagi_domain::plan_note::{OplogRestoreNote, PlanNote};
    use kagi_domain::restore_preview::RestorePreview;

    let fixture = build_fixture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));
    let mut backend = Backend::open(&repo).unwrap();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: "point".into(),
            at: head.clone(),
        },
    );
    let point_id = read_oplog_tail(1).pop().unwrap().id;
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateTag {
            name: "release".into(),
            at: head,
        },
    );

    // This tag is outside Kagi's record, and ordinary repositories do not
    // keep tag reflogs. It must survive the restore with an honest warning.
    git(
        &repo,
        &["-c", "core.logAllRefUpdates=false", "tag", "outside"],
    );
    assert!(!repo.join(".git/logs/refs/tags/outside").exists());
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        app.bottom_panel_height = 600.;
        cx.notify();
    });
    paint(cx, window);
    let rows = rows_of(cx, &app, &repo);
    assert_eq!(rows.len(), 2);
    click_row(cx, &app, window, rows[1]);
    click_probe(cx, window, &format!("oplog-restore-{}-enabled", rows[1]));
    let card = restore_card(cx, &app);
    assert_eq!(card.op, Operation::RestoreToPoint { entry_id: point_id });
    assert!(card.plan.blockers.is_empty(), "{:?}", card.plan.blockers);
    assert!(card.plan.warnings.iter().any(|warning| matches!(
        warning,
        PlanNote::OplogRestore(OplogRestoreNote::Moves { refname, to: None, .. })
            if refname == "refs/tags/release"
    )));
    assert!(card
        .restores
        .iter()
        .any(|row| row.refname == "refs/tags/release" && row.restore_to.is_none()));
    paint(cx, window);
    assert!(painted(window, "restore-ref-0"));
    let valid_plan = (*card.plan).clone();
    assert_eq!(card.preview.unwrap().graph, RestorePreview::TagChange);
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        paint(cx, window);
        assert!(painted(window, "restore-preview-unavailable"));
        assert!(!painted(window, "plan-recovery-scroll"));
        click_probe(cx, window, "plan-card-copy");
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert!(copied.contains("refs/tags/release"), "{copied}");
        assert!(copied.contains("git update-ref --stdin"), "{copied}");
    }
    assert!(restore_card(cx, &app)
        .plan
        .equivalent_command
        .as_deref()
        .is_some_and(|command| command.starts_with("git update-ref --stdin")));
    i18n::set_lang(original_language);
    confirm_twice(cx, &app, window);
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/release"]
    ));
    assert!(git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/outside"]
    ));
    assert_eq!(read_oplog_tail(1).pop().unwrap().op, "restore-to-point");
    // Malformed canonical ref rows fail closed at the real admission seam.
    // The receipt retains the full error; users see localized, bounded text.
    let bad_row = format!("invalid-restore-row-{}", "x".repeat(300));
    let refs_before = git_output(&repo, &["show-ref"]);
    for (language, invalid, no_repo, missing_session) in [
        (
            i18n::Lang::En,
            "Restore plan ref rows are invalid; see the Operation Log",
            "Restore plan ref rows are invalid; no repository is open",
            "repo session unavailable",
        ),
        (
            i18n::Lang::Ja,
            "復元計画の ref 行が不正です。詳細は Operation Log を確認してください",
            "復元計画の ref 行が不正です。リポジトリは開かれていません",
            "リポジトリのセッションを利用できません",
        ),
    ] {
        i18n::set_lang(language);
        let mut malformed = valid_plan.clone();
        malformed.preview_commits = vec![bad_row.clone()];
        let previous_id = read_oplog_tail(1).pop().unwrap().id;
        app.update(cx, |app, cx| {
            app.admit_oplog_restore_plan_for_test(
                Operation::RestoreToPoint { entry_id: point_id },
                malformed.clone(),
                cx,
            )
        });
        assert!(cx.read(|cx| app.read(cx).oplog_restore_modal().is_none()));
        assert_eq!(git_output(&repo, &["show-ref"]), refs_before);
        let receipt = read_oplog_tail(1).pop().expect("failed receipt persisted");
        assert_ne!(receipt.id, previous_id);
        let OpOutcome::Failed { error } = &receipt.outcome else {
            panic!(
                "malformed plan must have a failed receipt: {:?}",
                receipt.outcome
            )
        };
        assert!(error.contains(&bad_row), "{error}");
        cx.read(|cx| {
            let state = app.read(cx);
            let kagi::ui::FooterStatus::Failed(footer) = &state.status_footer else {
                panic!("malformed plan must show failed footer")
            };
            assert_eq!(footer.as_ref(), invalid);
            let toast = state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert_eq!(toast.kind, kagi::ui::ToastKind::Error);
            assert_eq!(toast.message.as_ref(), invalid);
        });

        // A missing session reports a localized plan failure rather than
        // leaking an English inner reason into the translated footer.
        app.update(cx, |app, cx| {
            let session = app.ui_mut().unwrap().repo_session.take().unwrap();
            app.open_oplog_restore_modal(Operation::RestoreToPoint { entry_id: point_id }, cx);
            app.ui_mut().unwrap().repo_session = Some(session);
        });
        cx.read(|cx| {
            let state = app.read(cx);
            let kagi::ui::FooterStatus::Failed(footer) = &state.status_footer else {
                panic!("missing session must show failed footer")
            };
            assert!(footer.ends_with(missing_session), "{footer}");
        });

        // Without a repository path no receipt can be stored, so the toast
        // itself must explain the malformed plan in the active language.
        app.update(cx, |app, cx| {
            let path = app.repo_path.take().unwrap();
            app.admit_oplog_restore_plan_for_test(
                Operation::RestoreToPoint { entry_id: point_id },
                malformed,
                cx,
            );
            app.repo_path = Some(path);
        });
        cx.read(|cx| {
            let state = app.read(cx);
            let toast = state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert_eq!(toast.kind, kagi::ui::ToastKind::Error);
            assert_eq!(toast.message.as_ref(), no_repo);
        });
        assert_eq!(read_oplog_tail(1).pop().unwrap().id, receipt.id);
        assert_eq!(git_output(&repo, &["show-ref"]), refs_before);
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS oplog_restore_tag_preview: EN/JA card warns about unrecorded tag changes; confirming removes recorded tag and leaves external tag untouched");
}

/// #883 review: a long preview, with branch Solo on and a fetched PR head.
/// - Solo filters the graph only: the preview still counts from every loaded
///   row (a deleted branch's own commit and main's newest commit leave).
/// - A commit only a `refs/kagi/pr/**` head still reaches stays.
/// - The rows scroll in their own capped box (the card body does not), so
///   the last row can be reached.
/// - `Copy all` carries the preview.
pub fn scenario_oplog_restore_preview_review(cx: &mut VisualTestAppContext) {
    use kagi_domain::restore_preview::RestorePreview;
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // Explicit, increasing commit dates: the graph orders by date within the
    // topology, so a side commit dated just after its parent is drawn right
    // above it — deep in the list, far from main's tip.
    let commit_at = |secs: i64, msg: &str| {
        let date = format!("@{secs} +0000");
        let ok = git_command(&repo)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args(["commit", "-q", "--allow-empty", "-m", msg])
            .status()
            .unwrap()
            .success();
        assert!(ok, "commit {msg}");
    };
    const BASE: i64 = 978_307_200; // 2001-01-01
    for n in 0..30 {
        commit_at(BASE + n * 100, &format!("c{n}"));
    }
    // A commit hanging deep in main's history, and one only a PR head keeps.
    let side_commit = |base: &str, secs: i64, msg: &str| -> String {
        git(&repo, &["checkout", "-q", "-b", "tmp", base]);
        commit_at(secs, msg);
        let oid = git_output(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["branch", "-q", "-D", "tmp"]);
        oid
    };
    let deep = side_commit("HEAD~28", BASE + 150, "deep");
    let pr_head = side_commit("HEAD~1", BASE + 2850, "pr head");
    git(&repo, &["update-ref", "refs/kagi/pr/7/head", &pr_head]);
    let before = git_output(&repo, &["rev-parse", "main"]);

    let mut backend = Backend::open(&repo).unwrap();
    let create = |backend: &mut Backend, name: &str, at: &str| {
        run(
            backend,
            Actor::Human,
            Operation::CreateBranch {
                name: name.into(),
                at: CommitId(at.into()),
            },
        )
    };
    create(&mut backend, "mark", &before);
    create(&mut backend, "deep", &deep);
    create(&mut backend, "prb", &pr_head);
    std::fs::write(repo.join("README.md"), "# fixture\nafter mark\n").unwrap();
    git(&repo, &["add", "README.md"]);
    run(
        &mut backend,
        Actor::Human,
        Operation::Commit {
            message: "after mark".into(),
        },
    );

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.toggle_branch_solo("mark".into(), CommitId(before.clone()), cx);
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        app.bottom_panel_height = 600.;
        cx.notify();
    });
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).view().branch_solo.is_some()),
        "precondition: Solo is on"
    );
    let rows = rows_of(cx, &app, &repo);
    let mark = rows[3];
    click_row(cx, &app, window, mark);
    click_probe(cx, window, &format!("oplog-restore-{mark}-enabled"));
    let card = restore_card(cx, &app);
    assert!(card.plan.blockers.is_empty(), "{:?}", card.plan.blockers);
    let preview = card.preview.clone().expect("preview");
    let RestorePreview::Graph {
        rows: drawn,
        removed,
        ..
    } = &preview.graph
    else {
        panic!("{:?}", preview.graph)
    };
    assert_eq!(
        *removed, 2,
        "deep's commit and main's newest leave; the PR head's commit stays (Solo ignored)"
    );
    assert!(drawn.len() > 20, "a long window: {}", drawn.len());

    // The rows sit in their own capped, scrolling box.
    paint(cx, window);
    let id = window.window_id();
    let list = e2e::control_bounds(id, "restore-preview-rows").expect("rows box");
    let last = format!("restore-preview-row-{}", drawn.len() - 1);
    let row_h = e2e::control_bounds(id, "restore-preview-row-0")
        .unwrap()
        .size
        .height;
    assert!(
        list.size.height < row_h * drawn.len() as f32,
        "the box is capped: {list:?} for {} rows",
        drawn.len()
    );
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    e2e::clear_control_bounds(id, &last);
    paint(cx, window);
    let last_row = e2e::control_bounds(id, &last).expect("last row painted");
    assert!(
        last_row.bottom() <= list.bottom() + gpui::px(1.),
        "scrolled, the last row is inside the box: {last_row:?} vs {list:?}"
    );

    // Copy all carries the preview.
    click_probe(cx, window, "plan-card-copy");
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("copied text");
    assert!(
        copied.contains(&kagi::ui::i18n::oplog_panel::preview_heading(2)),
        "{copied}"
    );
    assert!(copied.contains("[main ←"), "{copied}");
    // One Copy all wording check: the same off-branch commit must carry a
    // localized marker, not English embedded in Japanese clipboard text.
    let off_branch = drawn
        .iter()
        .find(|row| row.off_branch)
        .expect("restore leaves a commit off every branch")
        .id
        .short();
    for (language, marker) in [
        (i18n::Lang::En, " (off branch)"),
        (i18n::Lang::Ja, "（どの branch からも外れます）"),
    ] {
        i18n::set_lang(language);
        paint(cx, window);
        click_probe(cx, window, "plan-card-copy");
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("localized Copy all text");
        let ghost = copied
            .lines()
            .find(|line| line.starts_with(&format!("  {off_branch}")))
            .expect("off-branch commit in Copy all");
        assert!(ghost.contains(marker), "{language:?}: {ghost}");
        if language == i18n::Lang::Ja {
            assert!(!copied.contains("(off branch)"), "{copied}");
        }
    }
    i18n::set_lang(original_language);

    press_key(cx, &app, window, "escape");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS oplog_restore_preview_review: Solo on, PR head kept: removed=2; the rows scroll in a capped box to the last row; Copy all carries the preview");
}

/// #884: a merge resolved in Kagi no longer stops a restore. The merge, the
/// conflict save (production `run_recorded_conflict`) and the merge commit
/// are all recorded, so restoring to the point before the merge plans
/// without blockers and puts main back.
pub fn scenario_oplog_restore_across_merge(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-qb", "side"]);
    std::fs::write(repo.join("README.md"), "# fixture\nside\n").unwrap();
    git(&repo, &["commit", "-qam", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "# fixture\nmain\n").unwrap();
    git(&repo, &["commit", "-qam", "main"]);
    let before_merge = git_output(&repo, &["rev-parse", "main"]);

    let mut backend = Backend::open(&repo).unwrap();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: "mark".into(),
            at: CommitId(before_merge.clone()),
        },
    );
    run(
        &mut backend,
        Actor::Human,
        Operation::MergeIntoConflict {
            target: "side".into(),
        },
    );
    let snapshot = backend
        .conflict_snapshot()
        .unwrap()
        .expect("merge in conflict");
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer
        .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Current)
        .unwrap();
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("README.md"),
        snapshot.observation.kind,
        "",
    )
    .unwrap();
    let plan = Backend::plan_recorded_conflict(&repo, request).unwrap();
    Backend::run_recorded_conflict(
        &plan,
        kagi_git::backend::ExecutionPolicy::human(false),
        None,
    );
    run(
        &mut backend,
        Actor::Human,
        Operation::MergeCommit {
            message: "merge side".into(),
        },
    );
    assert_ne!(git_output(&repo, &["rev-parse", "main"]), before_merge);

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        app.bottom_panel_height = 600.;
        cx.notify();
    });
    paint(cx, window);
    // Newest first: merge-commit, conflict save, merge, create-branch.
    let rows = rows_of(cx, &app, &repo);
    assert_eq!(rows.len(), 4, "{rows:?}");
    let mark = rows[3];
    click_row(cx, &app, window, mark);
    click_probe(cx, window, &format!("oplog-restore-{mark}-enabled"));
    let card = restore_card(cx, &app);
    assert!(
        card.plan.blockers.is_empty(),
        "every entry across the merge is recorded: {:?}",
        card.plan.blockers
    );
    confirm_twice(cx, &app, window);
    assert_eq!(
        git_output(&repo, &["rev-parse", "main"]),
        before_merge,
        "main is back before the merge"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS oplog_restore_across_merge: merge → conflict save → merge commit are recorded; restoring to before the merge has no blockers and puts main back");
}
