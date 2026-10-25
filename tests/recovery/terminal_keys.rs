//! Real PTY clipboard bytes and production keyboard focus routing (#1403/#1406/#1407).
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, ClipboardItem, Entity, VisualTestAppContext};
use kagi::ui::{
    i18n::{self, Lang},
    theme, BottomTab, KagiApp,
};
use std::os::unix::fs::PermissionsExt;

struct CaptureShell {
    dir: tempfile::TempDir,
}
impl CaptureShell {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("capture.py");
        std::fs::write(
            &program,
            format!(
                r#"import os, tty, pathlib
root = pathlib.Path({:?})
tty.setraw(0)
os.write(1, b'\x1b[?20')
os.write(1, b'04h')
(root / 'ready').touch()
while True:
    try:
        data = os.read(0, 4096)
    except OSError:
        break
    if not data:
        break
    if data == b'\x04':
        os.write(1, b'\x1b[?2004l')
        (root / 'disabled').touch()
    elif data == b'\x05':
        break
    else:
        with (root / 'input').open('ab') as f:
            f.write(data)
"#,
                dir.path().to_str().unwrap()
            ),
        )
        .unwrap();
        let shell = dir.path().join("shell.sh");
        std::fs::write(
            &shell,
            format!("#!/bin/sh\nexec /usr/bin/python3 '{}'\n", program.display()),
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        KagiApp::set_terminal_shell_for_e2e(Some(shell.display().to_string()));
        Self { dir }
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("input")).unwrap_or_default()
    }
}
impl Drop for CaptureShell {
    fn drop(&mut self) {
        KagiApp::set_terminal_shell_for_e2e(None);
    }
}

fn settle(cx: &mut VisualTestAppContext, condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if condition() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PTY did not deliver expected bytes"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Let the emulator consume output queued immediately before the handshake.
    std::thread::sleep(std::time::Duration::from_millis(50));
    cx.run_until_parked();
}
fn focus_terminal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::Terminal;
            app.ensure_terminal(window, cx);
            cx.notify();
        });
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn terminal_focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) -> bool {
    cx.update_window(win, |_, window, cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .read(cx)
            .focus_handle()
            .is_focused(window)
    })
    .unwrap()
}
fn paste(cx: &mut VisualTestAppContext, win: AnyWindowHandle, text: &str) {
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.into())));
    crate::keyboard_nav::keys(cx, win, "cmd-v");
}
fn matrix(
    cx: &mut VisualTestAppContext,
    exercise: impl Fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle, &CaptureShell),
) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _ports = crate::gui_isolation::PortStore::keep();
    let fixture = build_fixture();
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for (width, height) in [(940., 660.), (1392., 883.)] {
                i18n::set_lang(lang);
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                let shell = CaptureShell::new();
                let (app, win) = mount(cx, fixture.path());
                cx.update_window(win, |_, window, _| {
                    window.resize(gpui::size(gpui::px(width), gpui::px(height)))
                })
                .unwrap();
                focus_terminal(cx, &app, win);
                settle(cx, || shell.dir.path().join("ready").exists());
                exercise(cx, &app, win, &shell);
                stop_capture_shell(cx, &app);
                unmount(cx, app, win);
            }
        }
    }
}
fn stop_capture_shell(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .write_input("\x05");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .unwrap()
                .view
                .is_none()
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "capture shell did not exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn assert_stale_paste_rejected(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    shell: &CaptureShell,
    text: &str,
) {
    focus_terminal(cx, app, win);
    paste(cx, win, text);
    let generation = cx.read(|cx| app.read(cx).terminal_paste_modal().unwrap().generation);
    let before = shell.bytes();
    stop_capture_shell(cx, app);
    std::fs::remove_file(shell.dir.path().join("ready")).unwrap();
    focus_terminal(cx, app, win);
    settle(cx, || shell.dir.path().join("ready").exists());
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns),
        generation + 1
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).terminal_paste_modal().unwrap().generation),
        generation
    );
    crate::app_conflict::click_control(cx, win, "terminal-paste-confirm");
    std::thread::sleep(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).terminal_paste_modal().is_none()));
    assert_eq!(
        shell.bytes(),
        before,
        "approval from the exited shell must not reach its replacement"
    );
    focus_terminal(cx, app, win);
    paste(cx, win, "fresh");
    settle(cx, || shell.bytes().len() > before.len());
    assert_eq!(
        &shell.bytes()[before.len()..],
        b"\x1b[200~fresh\x1b[201~",
        "the replacement PTY remains usable after stale approval is discarded"
    );
}

pub fn scenario_terminal_paste_bracketed(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, win, shell| {
        paste(cx, win, "one\ntwo\r\n日本語");
        settle(cx, || !shell.bytes().is_empty());
        assert_eq!(
            shell.bytes(),
            b"\x1b[200~one\ntwo\r\n\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e\x1b[201~",
            "enabled bracketed paste must protect every clipboard line"
        );
        let offset = shell.bytes().len();
        paste(cx, win, "safe\x1b[201~\necho injected\x03\r\n日本語");
        settle(cx, || shell.bytes().len() > offset);
        assert_eq!(
            &shell.bytes()[offset..],
            b"\x1b[200~safe[201~\necho injected\r\n\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e\x1b[201~",
            "bracketed paste must remove embedded ESC and ETX without changing CR/LF"
        );
        cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .unwrap()
                .paste_writer
                .as_ref()
                .unwrap()
                .write_input("\x04")
        });
        settle(cx, || shell.dir.path().join("disabled").exists());
        let before = shell.bytes();
        paste(cx, win, "unsafe\t\ncommand\n");
        cx.run_until_parked();
        assert_eq!(
            shell.bytes(),
            before,
            "disabled multi-line paste must await explicit confirmation"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).terminal_paste_description_for_e2e().unwrap()),
            i18n::Msg::TerminalPasteBody.t(),
            "clipboard Tab must not hide the newline warning"
        );
        crate::keyboard_nav::keys(cx, win, "enter");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .terminal_paste_modal()
                .unwrap()
                .text
                .to_string()),
            "unsafe\t\ncommand\n"
        );
        assert_eq!(shell.bytes(), before, "Return must send no clipboard bytes");
        crate::keyboard_nav::keys(cx, win, "escape");
        assert_eq!(shell.bytes(), before, "Cancel must send no clipboard bytes");
        focus_terminal(cx, app, win);
        paste(cx, win, "approved\ncommand\n");
        crate::app_conflict::click_control(cx, win, "terminal-paste-confirm");
        settle(cx, || shell.bytes().len() > before.len());
        assert_eq!(&shell.bytes()[before.len()..], b"approved\ncommand\n");
        assert_stale_paste_rejected(cx, app, win, shell, "stale\ncommand\n");
    });
}

pub fn scenario_terminal_paste_control(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, win, shell| {
        cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .unwrap()
                .paste_writer
                .as_ref()
                .unwrap()
                .write_input("\x04")
        });
        settle(cx, || shell.dir.path().join("disabled").exists());
        for text in ["A\x03B", "\x1b[31m日本語", "A\x00B", "A\x7fB"] {
            focus_terminal(cx, app, win);
            let before = shell.bytes();
            paste(cx, win, text);
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            assert_eq!(
                shell.bytes(),
                before,
                "plain paste with invisible control characters must await approval: {text:?}"
            );
            assert_eq!(
                cx.read(|cx| app
                    .read(cx)
                    .terminal_paste_modal()
                    .unwrap()
                    .text
                    .to_string()),
                text
            );
            assert_eq!(
                cx.read(|cx| app.read(cx).terminal_paste_description_for_e2e().unwrap()),
                i18n::Msg::TerminalPasteControlBody.t(),
                "the rendered and accessible warning must name invisible control characters"
            );
            crate::app_conflict::click_control(cx, win, "terminal-paste-cancel");
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).terminal_paste_modal().is_none()));
            assert_eq!(shell.bytes(), before, "Cancel must send zero bytes");
        }
        focus_terminal(cx, app, win);
        let before = shell.bytes();
        paste(cx, win, "approved\x03\x1b[31m日本語");
        crate::app_conflict::click_control(cx, win, "terminal-paste-confirm");
        settle(cx, || shell.bytes().len() > before.len());
        assert_eq!(
            &shell.bytes()[before.len()..],
            "approved\x03\x1b[31m日本語".as_bytes(),
            "explicit approval sends the original clipboard exactly once"
        );
        for text in ["normal text 日本語", "A\tB"] {
            focus_terminal(cx, app, win);
            let before = shell.bytes();
            paste(cx, win, text);
            settle(cx, || shell.bytes().len() > before.len());
            assert!(cx.read(|cx| app.read(cx).terminal_paste_modal().is_none()));
            assert_eq!(
                &shell.bytes()[before.len()..],
                text.as_bytes(),
                "ordinary text and clipboard Tab must be sent exactly once without approval"
            );
        }
        assert_stale_paste_rejected(cx, app, win, shell, "stale\x03command");
    });
}
pub fn scenario_terminal_focus_exit(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, win, shell| {
        crate::keyboard_nav::keys(cx, win, "tab shift-tab");
        settle(cx, || shell.bytes().len() >= 4);
        assert_eq!(
            shell.bytes(),
            b"\t\x1b[Z",
            "completion keys remain raw PTY input"
        );
        crate::keyboard_nav::keys(cx, win, "ctrl-shift-tab");
        assert!(
            !terminal_focused(cx, app, win),
            "Ctrl-Shift-Tab must leave captured terminal focus"
        );
        assert!(
            cx.read(|cx| app.read(cx).bottom_panel_open),
            "focus exit must not hide terminal"
        );
        assert_eq!(
            cx.update_window(win, |_, window, cx| app
                .read(cx)
                .bottom_tab_focused_for_e2e(window))
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .terminal_description_for_e2e()
                .unwrap()
                .to_string()),
            i18n::Msg::TerminalFocusDescription.t()
        );
        crate::keyboard_nav::keys(cx, win, "left enter");
        assert_eq!(
            cx.read(|cx| app.read(cx).bottom_tab),
            BottomTab::OperationLog,
            "exit must allow keyboard tab activation"
        );
        focus_terminal(cx, app, win);
        crate::keyboard_nav::keys(cx, win, "cmd-shift-j");
        assert!(
            !terminal_focused(cx, app, win),
            "Cmd-Shift-J must leave terminal focus"
        );
        assert_eq!(
            shell.bytes(),
            b"\t\x1b[Z",
            "exit shortcuts must not send bytes to PTY"
        );
    });
}
pub fn scenario_terminal_hide_focus(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, win, shell| {
        crate::keyboard_nav::keys(cx, win, "cmd-j");
        assert!(!cx.read(|cx| app.read(cx).bottom_panel_open));
        assert!(
            !terminal_focused(cx, app, win),
            "hiding Terminal must return focus to a visible owner"
        );
        crate::keyboard_nav::keys(cx, win, "x tab");
        assert!(
            shell.bytes().is_empty(),
            "hidden terminal must never receive subsequent typing or Tab"
        );
        let generation = cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns);
        crate::keyboard_nav::keys(cx, win, "cmd-j");
        assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns),
            generation,
            "reopening must retain the existing shell"
        );
    });
}
