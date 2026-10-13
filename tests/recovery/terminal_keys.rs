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
                unmount(cx, app, win);
            }
        }
    }
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
        paste(cx, win, "unsafe\ncommand\n");
        cx.run_until_parked();
        assert_eq!(
            shell.bytes(),
            before,
            "disabled multi-line paste must await explicit confirmation"
        );
        crate::keyboard_nav::keys(cx, win, "escape");
        assert_eq!(shell.bytes(), before, "Cancel must send no clipboard bytes");
        focus_terminal(cx, app, win);
        paste(cx, win, "approved\ncommand\n");
        crate::app_conflict::click_control(cx, win, "terminal-paste-confirm");
        settle(cx, || shell.bytes().len() > before.len());
        assert_eq!(&shell.bytes()[before.len()..], b"approved\ncommand\n");
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
