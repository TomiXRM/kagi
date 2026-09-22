//! Lock reason entry is not approval: only the reviewed plan may change Git.
use std::path::Path;
use std::process::Command;

use gpui::{AnyWindowHandle, ClipboardItem, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::recovery_operations::press_enter;

fn lock_reason(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
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
