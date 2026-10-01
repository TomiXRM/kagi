//! #334 (ADR-0214): the Operation Log panel reads — actor and worktree badges
//! on each row; a selected row shows the ref moves it recorded (slice 2a), or,
//! for an entry without a record, reflog lines estimated by its time window
//! with a shared second marked ambiguous (slice 1). Nothing is written.
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::oplog_panel::{entry_worktree, ReflogDetail};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::oplog_reflog::Attribution;
use kagi_domain::ref_moves::RefMove;
use kagi_git::oplog::{append_oplog, read_oplog_tail, Actor, OpLogEntry, OpOutcome};
use kagi_git::{Backend, CommitId, Operation, StateSummary};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

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
