//! #355 stage 3a: the operation queue wired to checkout (ADR-0204).
//!
//! A real checkout is admitted and held before its backend work
//! (`KagiApp::hold_next_run_for_e2e`); further checkouts are queued through
//! the product's own entry (`dblclick_checkout_branch` → `start_checkout`).
//! The clock is the test dispatcher's, advanced in the queue's 250 ms ticks.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::WindowExt as _;
use kagi::ui::{e2e, KagiApp};

use crate::app_conflict::click_control;
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};

const TICK: Duration = Duration::from_millis(250);

/// `main` with branches `a`, `b` and `c` at its tip: each checkout is clean.
fn branches_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    for branch in ["a", "b", "c"] {
        git(fixture.path(), &["branch", branch]);
    }
    fixture
}

/// The checked-out branch name.
fn head(repo: &Path) -> String {
    rev_parse(repo, &["--abbrev-ref", "HEAD"])
}

fn rev_parse(repo: &Path, args: &[&str]) -> String {
    let mut command = vec!["rev-parse"];
    command.extend_from_slice(args);
    git_output(repo, &command)
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn advance(cx: &mut VisualTestAppContext, ticks: u32) {
    for _ in 0..ticks {
        cx.advance_clock(TICK);
        cx.run_until_parked();
    }
}

/// Tick until `done` holds; the queue moves only on observed events.
fn tick_until(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    what: &str,
    done: impl Fn(&KagiApp) -> bool,
) {
    for _ in 0..80 {
        cx.run_until_parked();
        if cx.read(|cx| done(app.read(cx))) {
            return;
        }
        cx.advance_clock(TICK);
    }
    panic!("timed out waiting for {what}");
}

type Strip = (usize, Vec<(String, String)>, Vec<(String, String)>);

fn strip(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<Strip> {
    let now = cx.background_executor.now();
    cx.read(|cx| app.read(cx).queue_strip_for_e2e(now))
}

fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) -> bool {
    let id = window.window_id();
    e2e::clear_control_bounds(id, name);
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    e2e::control_bounds(id, name).is_some()
}

fn klog_index(line: &str) -> Option<usize> {
    kagi_ui_core::klog::tail()
        .into_iter()
        .rposition(|logged| logged == line)
}

fn rows(strip: &Option<Strip>) -> Vec<(String, String)> {
    strip.as_ref().map(|s| s.1.clone()).unwrap_or_default()
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} control was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
}

/// Q1: a checkout queued behind a running one waits, then runs in order —
/// with no modal, because its replan is clean — and its strip row shows the
/// running write's elapsed seconds.
pub fn scenario_queue_runs_in_order(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.app_sessions.has_leases(), "checkout a is admitted");
        assert_eq!(state.write_busy_op, Some("checkout"));
    });

    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    let shown = strip(cx, &app).expect("the strip appears with the intent");
    assert_eq!(shown.0, 1, "queued: 1");
    assert_eq!(
        shown.1,
        vec![("checkout b".to_string(), "waiting: write".to_string())]
    );
    assert!(drawn(cx, window, "queue-strip"), "the strip is drawn");
    cx.read(|cx| {
        let state = app.read(cx);
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert!(
            toasts
                .iter()
                .any(|t| t.message.as_ref() == "Queued: checkout b"),
            "a short toast names the queued intent"
        );
        assert!(!e2e::active_modal_present(state), "queuing opens nothing");
    });
    assert_eq!(head(&repo), "main", "nothing ran yet");

    // Every waiting row can be taken out on its own; the rest stay.
    app.update(cx, |app, cx| app.dblclick_checkout_branch("c", cx));
    cx.run_until_parked();
    let c = *cx
        .read(|cx| app.read(cx).queue_ids_for_e2e())
        .last()
        .unwrap();
    click(cx, window, &format!("queue-strip-remove-{c}"));
    cx.run_until_parked();
    let shown = strip(cx, &app).unwrap();
    assert_eq!(shown.0, 1);
    assert_eq!(
        shown.1,
        vec![("checkout b".to_string(), "waiting: write".to_string())]
    );
    assert_eq!(
        shown.2,
        vec![("checkout c".to_string(), "removed".to_string())]
    );

    let (second_hold, second_release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(second_hold);
    release.send(());
    tick_until(cx, &app, "b to be admitted after a", |app| {
        rows(&app.queue_strip_for_e2e(std::time::Instant::now()))
            .first()
            .is_some_and(|(_, state)| state.starts_with("running"))
    });
    assert_eq!(head(&repo), "a", "a finished first");
    let finished_a = klog_index("[kagi] async: checkout finished").expect("a finished");
    let ran_b = klog_index("[kagi] queue: run checkout b (clean plan)").expect("b ran");
    assert!(finished_a < ran_b, "b started only after a settled");
    cx.read(|cx| {
        assert!(
            !e2e::active_modal_present(app.read(cx)),
            "clean replan: no modal"
        )
    });

    advance(cx, 8);
    let shown = strip(cx, &app).expect("b is still running");
    assert_eq!(shown.0, 0, "a running write is not queued");
    assert_eq!(
        shown.1,
        vec![("checkout b".to_string(), "running · 2 s".to_string())]
    );

    second_release.send(());
    tick_until(cx, &app, "b to settle", |app| {
        rows(&app.queue_strip_for_e2e(std::time::Instant::now())).is_empty()
    });
    assert_eq!(head(&repo), "b");
    assert!(
        klog_index("[kagi] queue: run checkout c (clean plan)").is_none(),
        "the removed intent never ran"
    );
    click(cx, window, "queue-strip-clear");
    cx.run_until_parked();
    assert!(strip(cx, &app).is_none());
    assert!(
        !drawn(cx, window, "queue-strip"),
        "an empty queue draws nothing"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_runs_in_order");
}

/// Q2: the running checkout fails, so both checkouts queued behind it are
/// cancelled and listed with the reason until the user clears them.
pub fn scenario_queue_trip_lists_cancelled(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    app.update(cx, |app, cx| app.dblclick_checkout_branch("c", cx));
    cx.run_until_parked();
    assert_eq!(strip(cx, &app).expect("queued").0, 2);

    // a's branch disappears after its plan: the held checkout fails.
    git(&repo, &["branch", "-D", "a"]);
    release.send(());
    tick_until(cx, &app, "the chain to trip", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 2)
    });
    let shown = strip(cx, &app).unwrap();
    assert_eq!(
        shown.2,
        vec![
            ("checkout c".to_string(), "previous step failed".to_string()),
            ("checkout b".to_string(), "previous step failed".to_string()),
        ]
    );
    assert_eq!(head(&repo), "main", "neither successor ran");
    assert!(drawn(cx, window, "queue-strip-clear"));

    // The list stays until cleared, across further ticks.
    advance(cx, 8);
    assert_eq!(strip(cx, &app).unwrap().2.len(), 2);
    click_control(cx, window, "queue-strip-clear");
    cx.run_until_parked();
    assert!(strip(cx, &app).is_none(), "cleared");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_trip_lists_cancelled");
}

/// A replan with a warning asks, as a checkout always does: confirming runs
/// it, declining cancels it and lists it as declined.
pub fn scenario_queue_confirms_a_warned_plan(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    app.update(cx, |app, cx| app.dblclick_checkout_branch("c", cx));
    // Local changes: every later checkout plan carries a warning.
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    release.send(());
    let queued_modal = |app: &KagiApp| app.plan_modal().and_then(|m| m.queued).is_some();
    tick_until(cx, &app, "b's confirmation", queued_modal);
    assert_eq!(head(&repo), "a");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![
            ("checkout b".to_string(), "confirming".to_string()),
            ("checkout c".to_string(), "queued".to_string()),
        ]
    );
    app.update(cx, |app, cx| app.start_checkout(cx));
    tick_until(cx, &app, "c's confirmation after b", |app| {
        queued_modal(app) && head_is(app, "b")
    });
    assert_eq!(head(&repo), "b");

    app.update(cx, |app, _| app.cancel_modal());
    tick_until(cx, &app, "the decline to land", |app| {
        rows(&app.queue_strip_for_e2e(std::time::Instant::now())).is_empty()
    });
    assert_eq!(
        strip(cx, &app).unwrap().2,
        vec![("checkout c".to_string(), "declined".to_string())]
    );
    assert_eq!(head(&repo), "b", "the declined checkout never ran");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_confirms_a_warned_plan");
}

fn head_is(app: &KagiApp, branch: &str) -> bool {
    app.view().status_summary.branch == branch
}

/// Q3 (+ the background half of Q10): tab B never shows tab A's intents, and
/// A's head waits for A to be on screen before it runs.
pub fn scenario_queue_strip_owner_only(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    assert!(drawn(cx, window, "queue-strip"));

    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    assert!(strip(cx, &app).is_none(), "B has no queue of its own");
    assert!(
        !drawn(cx, window, "queue-strip"),
        "A's intent is not drawn on B"
    );

    release.send(());
    tick_until(cx, &app, "a to finish in the background", |app| {
        !app.app_sessions.has_leases()
    });
    advance(cx, 4);
    assert_eq!(head(&repo), "a");
    // The queue hears of A's return in the same turn as the switch, while A's
    // activation read is still out: revalidation must reach it before the
    // owner does, or the head replans against the stale read.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.sync_queue_for_e2e(cx);
    });
    let rows = rows(&strip(cx, &app));
    assert!(
        rows == vec![("checkout b".to_string(), "waiting: confirm".to_string())],
        "A's head waits for A's read: {rows:?}"
    );
    assert_eq!(head(&repo), "a", "nothing ran on the stale read");
    tick_until(cx, &app, "b to run on return", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_strip_owner_only");
}

/// A quiet fetch never runs while the tab has queued intents.
pub fn scenario_queue_skips_auto_fetch(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    // A confirmation the queue must not take (one a reload does not sweep):
    // b waits for the slot.
    let at = kagi_git::CommitId(rev_parse(&repo, &["HEAD"]));
    app.update(cx, |app, cx| app.open_create_branch_modal(at, cx));
    release.send(());
    tick_until(cx, &app, "a to finish", |app| {
        !app.app_sessions.has_leases()
    });
    advance(cx, 4);
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".to_string(), "waiting: confirm".to_string())]
    );

    let skipped = || {
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|l| *l == "[kagi] auto-fetch: skipped while operations are queued")
            .count()
    };
    let before = skipped();
    app.update(cx, |app, cx| app.fetch_async(true, cx));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.fetch_in_flight.is_none(), "no fetch beside the queue");
        assert!(!state.app_sessions.has_leases());
    });
    assert_eq!(skipped(), before + 1);

    app.update(cx, |app, _| app.cancel_create_branch_modal());
    tick_until(cx, &app, "b to run", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    app.update(cx, |app, cx| app.fetch_async(true, cx));
    cx.read(|cx| {
        assert!(
            app.read(cx).fetch_in_flight.is_some(),
            "an empty queue lets the quiet fetch run"
        )
    });
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_skips_auto_fetch");
}

/// The tab's own guard writer (a manual fetch) has no judgeable receipt: a
/// checkout behind it is refused as before, not queued as a new chain.
pub fn scenario_queue_rejects_during_untracked_write(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    app.update(cx, |app, cx| app.fetch_async(false, cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(strip(cx, &app).is_none(), "nothing was queued");
    assert!(klog_index("[kagi] queue: rejected checkout a (UntrackedWrite)").is_some());
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            matches!(&state.status_footer, kagi::ui::types::FooterStatus::Idle(m)
                if m.as_ref() == "another operation is in progress"),
            "the old refusal stands"
        );
    });
    release.send(());
    cx.run_until_parked();
    assert_eq!(head(&repo), "main");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_rejects_during_untracked_write");
}

/// A tab whose chain ended in a reconcile is not stuck once the reconcile is
/// acknowledged: a later queued checkout runs (#1018 review P1).
pub fn scenario_queue_resumes_after_reconcile(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    // a's worker dies: Unknown, reconcile parked, b behind it is cancelled.
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    KagiApp::panic_next_run_for_e2e();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    release.send(());
    tick_until(cx, &app, "the chain to trip on Unknown", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 1)
    });
    app.update(cx, |app, cx| {
        kagi::ui::e2e::poll_app_jobs(app, cx);
        let ids = app.app_sessions.reconcile_ids();
        assert_eq!(ids.len(), 1, "the dead worker parks one reconcile");
        let read = kagi::app::read_reconcile(&app.app_sessions, ids[0]).expect("readable");
        kagi::app::acknowledge(&mut app.app_sessions, read).expect("acknowledged");
        // The reconcile notice holds the modal slot until the user closes it.
        while kagi::ui::e2e::active_modal_present(app) {
            app.clear_app_notice();
            kagi::ui::e2e::present_app_notice(app);
        }
    });
    tick_until(cx, &app, "the lease to go with the reconcile", |app| {
        !app.app_sessions.has_leases()
    });
    click(cx, window, "queue-strip-clear");
    cx.run_until_parked();

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("c", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert_eq!(strip(cx, &app).expect("queued").0, 1);
    release.send(());
    tick_until(cx, &app, "a to run after the reconcile", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "a");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_resumes_after_reconcile");
}

/// A fetch that ended while the queue was idle does not make the next write
/// look like the tab's own untrackable predecessor (#1018 review).
pub fn scenario_queue_accepts_after_idle_fetch(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.fetch_async(false, cx));
    cx.run_until_parked();
    assert!(
        !cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "fetch ended"
    );

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".to_string(), "waiting: write".to_string())],
        "queued behind the checkout, not refused for the old fetch"
    );
    release.send(());
    tick_until(cx, &app, "b to run", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_accepts_after_idle_fetch");
}

/// Confirming a blocked checkout while busy is refused, never queued.
pub fn scenario_queue_refuses_a_blocked_checkout(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // `blocked` changes the file the worktree is about to edit locally.
    git(&repo, &["checkout", "-q", "-b", "blocked"]);
    std::fs::write(repo.join("README.md"), "# fixture\nfrom blocked\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "blocked edit"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    let (app, window) = mount(cx, &repo);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, _| app.open_plan_modal("a"));
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "a is held"
    );

    app.update(cx, |app, _| app.open_plan_modal("blocked"));
    let blockers = cx.read(|cx| app.read(cx).plan_modal().map(|m| m.plan.blockers.len()));
    assert!(
        blockers.is_some_and(|n| n > 0),
        "precondition: a blocked plan"
    );
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    assert!(strip(cx, &app).is_none(), "a blocked plan is not queued");
    assert!(klog_index("[kagi] queue: enqueued checkout blocked").is_none());
    release.send(());
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_refuses_a_blocked_checkout");
}

/// Use the real Commit Panel inputs and plan button, not a synthetic queue event.
fn queue_commit(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    title: &str,
    body: &str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        let panel = app
            .read(cx)
            .ui()
            .commit_panel
            .clone()
            .expect("commit panel");
        let (title_input, body_input) = {
            let panel = panel.read(cx);
            (
                panel.title_input.clone().expect("title input"),
                panel.body_input.clone().expect("body input"),
            )
        };
        title_input.update(cx, |input, cx| input.set_value(title, window, cx));
        body_input.update(cx, |input, cx| input.set_value(body, window, cx));
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        let owner = app.active_session().expect("commit owner");
        app.open_commit_plan_modal(owner, cx);
    });
    cx.run_until_parked();
}

fn change_commit_title(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    title: &str,
) {
    cx.update_window(window, |_, window, cx| {
        let panel = app
            .read(cx)
            .ui()
            .commit_panel
            .clone()
            .expect("commit panel");
        let input = panel.read(cx).title_input.clone().expect("title input");
        input.update(cx, |input, cx| input.set_value(title, window, cx));
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn focus_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        window.focus(&root, cx);
        assert!(root.is_focused(window), "root owns focus after blur");
        window.draw(cx).clear();
    })
    .unwrap();
}

fn frozen_modal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    frozen: &str,
) {
    tick_until(cx, app, "queued commit confirmation", |app| {
        app.queued_commit_message_for_e2e().is_some()
    });
    assert_eq!(
        cx.read(|cx| app.read(cx).queued_commit_message_for_e2e()),
        Some(frozen.to_string()),
        "confirmation shows the entire frozen subject and body"
    );
    assert!(drawn(cx, window, "queued-commit-message"));
    assert!(drawn(cx, window, "plan-confirm"));
}

fn finish_queued_commit(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    tick_until(cx, app, "queued commit to settle", |app| {
        !app.app_sessions.has_leases()
            && app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
}

/// A clean queued commit uses the message and staged content selected behind
/// the held checkout, executes on its resulting branch, and verifies its HEAD.
pub fn scenario_queue_commit_runs_after_checkout(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = rev_parse(&repo, &["HEAD"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    std::fs::write(repo.join("first.txt"), "staged for a\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "frozen subject", "frozen body");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("commit frozen subject".into(), "waiting: confirm".into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    cx.read(|cx| {
        let toasts = app.read(cx).toast_stack.as_ref().unwrap().read(cx);
        assert!(toasts
            .toasts()
            .iter()
            .any(|toast| toast.message.as_ref() == "Queued: commit frozen subject"));
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), before);
    focus_root(cx, &app, window);
    release.send(());
    finish_queued_commit(cx, &app);
    assert_eq!(head(&repo), "a");
    assert_ne!(rev_parse(&repo, &["HEAD"]), before);
    assert_eq!(
        rev_parse(&repo, &["HEAD^{tree}:first.txt"]),
        rev_parse(&repo, &[":first.txt"])
    );
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "frozen subject\n\nfrozen body"
    );
    assert!(cx.read(|cx| app.read(cx).queued_commit_message_for_e2e().is_none()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_runs_after_checkout");
}

/// Q5b: editing the draft after queueing must ask, and approval must use the
/// old message while preserving the new text in the panel.
pub fn scenario_queue_commit_confirms_changed_draft(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "original subject", "original body");
    change_commit_title(cx, &app, window, "new draft");
    focus_root(cx, &app, window);
    release.send(());
    frozen_modal(cx, &app, window, "original subject\n\noriginal body");
    assert_eq!(head(&repo), "a");
    assert_eq!(rows(&strip(cx, &app))[0].1, "confirming");
    click(cx, window, "plan-confirm");
    finish_queued_commit(cx, &app);
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "original subject\n\noriginal body"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        let panel = state
            .ui()
            .commit_panel
            .as_ref()
            .expect("draft panel")
            .read(cx);
        assert_eq!(
            panel
                .title_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string(),
            "new draft",
            "confirming the frozen intent cannot consume the newer draft"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_confirms_changed_draft");
}

/// The staged-set digest is the frozen identity: a newly staged path asks for
/// approval even with an unchanged message and a clean live plan.
pub fn scenario_queue_commit_confirms_changed_staging(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "same subject", "same body");
    std::fs::write(repo.join("second.txt"), "second\n").unwrap();
    git(&repo, &["add", "second.txt"]);
    focus_root(cx, &app, window);
    release.send(());
    frozen_modal(cx, &app, window, "same subject\n\nsame body");
    assert_eq!(head(&repo), "a");
    assert_eq!(rows(&strip(cx, &app))[0].1, "confirming");
    click(cx, window, "plan-confirm");
    finish_queued_commit(cx, &app);
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "same subject\n\nsame body"
    );
    assert_eq!(
        rev_parse(&repo, &["HEAD^{tree}:second.txt"]),
        rev_parse(&repo, &[":second.txt"])
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_confirms_changed_staging");
}

/// A focused Input holds the queue even after the preceding write settles.
/// The next observation after a real blur admits the waiting head.
pub fn scenario_queue_waits_while_input_focused(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        let panel = app.read(cx).ui().commit_panel.clone().unwrap();
        let input = panel.read(cx).title_input.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
        assert!(
            window.has_focused_input(cx),
            "the real Commit Input owns focus"
        );
    })
    .unwrap();
    advance(cx, 2);
    release.send(());
    tick_until(cx, &app, "first checkout to settle", |app| {
        !app.app_sessions.has_leases()
    });
    advance(cx, 4);
    assert_eq!(head(&repo), "a", "b must not run while typing");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), "waiting: confirm".into())]
    );
    assert!(!cx.read(|cx| e2e::active_modal_present(app.read(cx))));
    focus_root(cx, &app, window);
    tick_until(cx, &app, "checkout b after input blur", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_waits_while_input_focused");
}
