//! #355 stages 3a–3b-2: checkout, commit and merge in the operation queue.
//!
//! Real entries enqueue behind held writes; the test dispatcher advances the
//! queue's 250 ms clock, and each confirmation runs through the normal modal.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::WindowExt as _;
use kagi::ui::{e2e, KagiApp};

use crate::app_conflict::click_control;
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};

#[path = "op_queue/checkout.rs"]
mod checkout;
#[path = "op_queue/commit.rs"]
mod commit;
#[path = "op_queue/merge.rs"]
mod merge;

pub use checkout::*;
use commit::queue_commit;
pub use commit::*;
pub use merge::*;

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
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        assert!(window.has_focused_input(cx), "modal input must own focus");
    })
    .unwrap();
    release.send(());
    tick_until(cx, &app, "a to finish", |app| {
        !app.app_sessions.has_leases()
    });
    advance(cx, 4);
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".to_string(), "waiting: typing".to_string())]
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

    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.run_until_parked();
    }
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

/// Dismissing an Input-bearing modal with Escape must release the queued
/// head even if GPUI still remembers the unmounted input's focus handle.
pub fn scenario_queue_dismissed_input_modal_releases_head(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    let at = kagi_git::CommitId(rev_parse(&repo, &["HEAD"]));
    app.update(cx, |app, cx| app.open_create_branch_modal(at, cx));
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        assert!(
            window.has_focused_input(cx),
            "create-branch Input has focus"
        );
        let modal = app.read(cx).create_branch_modal().expect("modal");
        let input = modal.input_state.as_ref().expect("real Input");
        let root = app.read(cx).root_focus.as_ref().expect("root focus");
        assert!(root.contains(&input.read(cx).focus_handle(cx), window));
    })
    .unwrap();
    release.send(());
    tick_until(cx, &app, "first checkout to settle", |app| {
        !app.app_sessions.has_leases()
    });
    assert_eq!(head(&repo), "a");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), "waiting: typing".into())]
    );

    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.run_until_parked();
    }
    tick_until(cx, &app, "checkout b after modal Escape", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_dismissed_input_modal_releases_head");
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
