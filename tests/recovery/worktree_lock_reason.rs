//! Lock reason entry is not approval: only the reviewed plan may change Git.
use std::path::Path;

use gpui::{AnyWindowHandle, ClipboardItem, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::recovery_operations::press_enter;

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

fn lock_reason(repo: &Path) -> Option<String> {
    git_output(repo, &["worktree", "list", "--porcelain", "-z"])
        .split('\0')
        .find_map(|line| {
            if line == "locked" {
                Some(String::new())
            } else {
                line.strip_prefix("locked ").map(str::to_owned)
            }
        })
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), id).expect("visible modal control");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn enter_reason(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    reason: &str,
) {
    app.update(cx, |app, cx| {
        app.open_lock_worktree_modal("linked".into());
        cx.notify();
    });
    click(cx, window, "worktree-lock-reason-input");
    cx.simulate_keystrokes(window, "cmd-a backspace");
    if !reason.is_empty() {
        app.update(cx, |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(reason.into()));
        });
        cx.simulate_keystrokes(window, "cmd-v");
    }
}

fn confirm_reason(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    reason: &str,
) {
    enter_reason(cx, app, window, reason);
    // Enter is delivered with the real InputState focused, without a manual
    // root-focus change or a render-time draft sync after the last input.
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_some()));
    assert_eq!(lock_reason(repo), None, "reviewing must not acquire a lock");
    paint(cx, window);
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert_eq!(lock_reason(repo).as_deref(), Some(reason));
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));
    let entries = read_oplog_tail_for_repo(repo, 100);
    let entry = entries
        .iter()
        .rev()
        .find(|entry| entry.op == "lock-worktree")
        .unwrap();
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
}

pub fn scenario_worktree_lock_reason(cx: &mut VisualTestAppContext) {
    let original_lang = i18n::lang();
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        let fixture = build_fixture();
        let linked_root = tempfile::tempdir().unwrap();
        let linked = linked_root.path().join("linked");
        git(
            fixture.path(),
            &[
                "worktree",
                "add",
                "-b",
                "reason-target",
                linked.to_str().unwrap(),
            ],
        );
        let before = repo_fingerprint(fixture.path());
        let (app, window) = mount(cx, fixture.path());

        enter_reason(cx, &app, window, "cancelled edit");
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).worktree_lock_reason_modal().is_none()));
        assert_eq!(lock_reason(fixture.path()), None);

        enter_reason(cx, &app, window, "cancelled plan");
        click(cx, window, "worktree-lock-reason-review");
        assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_some()));
        assert_eq!(lock_reason(fixture.path()), None);
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        cx.run_until_parked();
        assert_eq!(lock_reason(fixture.path()), None);

        confirm_reason(cx, &app, window, fixture.path(), "release \"QA\" — 作業中");
        app.update(cx, |app, cx| {
            app.open_unlock_worktree_modal("linked".into());
            cx.notify();
        });
        press_enter(cx, &app, window);
        cx.run_until_parked();
        assert_eq!(lock_reason(fixture.path()), None);
        confirm_reason(cx, &app, window, fixture.path(), "");
        assert_eq!(repo_fingerprint(fixture.path()), before);
        unmount(cx, app, window);
    }
    i18n::set_lang(original_lang);
    eprintln!("[gui-e2e] PASS worktree_lock_reason EN/JA input/plan cancellation, focused Enter, Unicode and blank reasons, Git porcelain and durable success");
}

/// #772 Phase 1 (ADR-0208): the terminal auto-lock is opt-in, and even when
/// on it only ever *offers* a plan. Mounted on a linked worktree:
/// - opt-in off: starting the terminal opens no card and locks nothing;
/// - opt-in on: starting the terminal opens the lock card with a Kagi token
///   reason and nothing is locked until Enter; the session owns the shell's
///   PID and spawn generation;
/// - the shell exiting is observed by the background wait (not the render
///   path): the session records the exit and the release card opens with
///   the token/identity-checked plan; a manual relock in between is refused
///   (contract B) and the manual lock survives.
pub fn scenario_terminal_auto_lock(cx: &mut VisualTestAppContext) {
    use kagi::ui::settings as theme;
    use kagi::ui::terminal::ShellExit;

    let fixture = build_fixture();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-b",
            "auto-target",
            linked.to_str().unwrap(),
        ],
    );
    let linked = linked.canonicalize().unwrap();
    let before = repo_fingerprint(fixture.path());
    let restore_auto = theme::terminal_auto_lock();

    // ── opt-in off ───────────────────────────────────────────────────────
    theme::set_terminal_auto_lock(false);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.lock_worktree_modal().is_none(),
            "opt-in off: no lock card"
        );
        let session = state
            .ui()
            .terminal_session
            .as_ref()
            .expect("terminal started");
        let shell = session.shell.as_ref().expect("the session owns its shell");
        assert_eq!(shell.generation, 1);
        assert!(shell.pid.is_some(), "a PID was recorded");
        assert!(shell.exit.is_none());
        assert!(session.auto_lock.is_none());
    });
    assert_eq!(
        lock_reason(fixture.path()),
        None,
        "opt-in off locked something"
    );
    unmount(cx, app, window);

    // ── opt-in on: offer, no write before confirm ────────────────────────
    theme::set_terminal_auto_lock(true);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let (pid, token_reason) = cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .lock_worktree_modal()
            .expect("opt-in on: the lock card opens");
        assert_eq!(modal.name, "linked");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert!(modal.reason.starts_with("kagi:auto:"), "{}", modal.reason);
        let session = state
            .ui()
            .terminal_session
            .as_ref()
            .expect("terminal started");
        let target = session
            .auto_lock
            .as_ref()
            .expect("the offer records its target");
        assert_eq!(target.token.reason(), modal.reason);
        assert!(target.worktree.git_dir.ends_with("worktrees/linked"));
        (
            session.shell.as_ref().unwrap().pid.unwrap(),
            modal.reason.clone(),
        )
    });
    assert_eq!(
        lock_reason(fixture.path()),
        None,
        "a lock was written before confirm"
    );
    // The shell's real cwd is the linked worktree (macOS/Linux probe).
    let probed = kagi_git::proc::cwd_of_pid(pid).expect("cwd probe");
    assert_eq!(probed.canonicalize().unwrap(), linked);

    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(token_reason.as_str())
    );
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));

    // ── shell exit → wait delivery → release offer ───────────────────────
    app.update(cx, |state, _| {
        let session = state.ui().terminal_session.as_ref().unwrap();
        session.paste_writer.as_ref().unwrap().paste_text("exit\n");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let exited = cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .and_then(|s| s.shell.as_ref())
                .and_then(|s| s.exit.clone())
        });
        if let Some(exit) = exited {
            assert!(matches!(exit, ShellExit::Exited { .. }), "{exit:?}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "shell exit was never delivered"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .unlock_worktree_modal()
            .expect("the release card opens on exit");
        assert_eq!(modal.name, "linked");
        assert!(modal.auto.is_some(), "the release is the auto-checked op");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
    });
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(token_reason.as_str())
    );

    // Someone relocks by hand between the offer and the confirm: refused, kept.
    git(
        fixture.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    git(
        fixture.path(),
        &[
            "worktree",
            "lock",
            "--reason",
            "taken over",
            linked.to_str().unwrap(),
        ],
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(lock_reason(fixture.path()).as_deref(), Some("taken over"));
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .unlock_worktree_modal()
            .expect("refusal keeps the card");
        let err = modal
            .error
            .as_ref()
            .expect("the refusal is shown")
            .to_string();
        assert!(err.contains("refused at preflight"), "{err}");
    });
    crate::recovery_operations::press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    git(
        fixture.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    assert_eq!(repo_fingerprint(fixture.path()), before);
    unmount(cx, app, window);
    theme::set_terminal_auto_lock(restore_auto);
    eprintln!("[gui-e2e] PASS terminal_auto_lock opt-in off/on, confirm-gated acquire, wait-delivered exit, refused release after a manual relock");
}
