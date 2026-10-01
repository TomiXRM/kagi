//! #855 (Refs #342): the port a worktree's terminal is given, end to end.
//!
//! The payload (`terminal_env`'s five values) is unit-tested; this proves the
//! shell the real PTY spawn starts actually *receives* it. The shell is a
//! script (`KagiApp::set_terminal_shell_for_e2e`) that writes its own
//! `KAGI_PORT` / `KAGI_WORKTREE_PATH` to a file, so the oracle is the spawned
//! process's environment, not anything Kagi computed. The value must be the
//! block the store holds for this worktree, and the sidebar row must then show
//! it as `localhost:<port>` — while a linked worktree that never had a
//! terminal (no stored block) shows nothing.

use std::path::{Path, PathBuf};

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};

use crate::macos::{build_fixture, git, mount, unmount};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn laid_out(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), id);
    for _ in 0..2 {
        draw(cx, window);
    }
    e2e::control_bounds(window.window_id(), id).is_some()
}

/// The drawn bounds of a measured control, after two fresh frames.
fn bounds(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(window.window_id(), id);
    for _ in 0..2 {
        draw(cx, window);
    }
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} was not laid out"))
}

/// A shell that records the environment it was started with, then waits on
/// its terminal (exits when the PTY closes at unmount).
fn recording_shell(dir: &Path, out: &Path) -> PathBuf {
    let script = dir.join("record-env.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf 'KAGI_PORT=%s\\nKAGI_WORKTREE_PATH=%s\\n' \
             \"$KAGI_PORT\" \"$KAGI_WORKTREE_PATH\" > '{}'\nexec cat\n",
            out.display()
        ),
    )
    .unwrap();
    std::process::Command::new("chmod")
        .args(["+x", script.to_str().unwrap()])
        .status()
        .unwrap();
    script
}

fn wait_for(path: &Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if text.ends_with('\n') {
                return text;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the terminal's shell never ran"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn start_terminal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

pub fn scenario_worktree_port_env(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let side_dir = tempfile::tempdir().unwrap();
    let side_root = side_dir.path().canonicalize().unwrap();
    let side = side_root.join("side");
    let pre = side_root.join("pre");
    for (name, path) in [("side", &side), ("pre", &pre)] {
        git(
            &repo,
            &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()],
        );
    }
    // `pre` already holds a block (as a worktree created by Kagi would): the
    // snapshot must read it back without a terminal ever opening there.
    let (start, end) = kagi::ui::settings::Settings::load().worktree_port_range();
    let per = kagi::ui::settings::Settings::load().worktree_ports_per_worktree();
    let pre_port = kagi_git::worktree_ports::assign_block(
        &pre,
        kagi_domain::worktree_ports::PortRange { start, end },
        per,
    )
    .expect("a free block for the pre-assigned worktree");
    let shell_dir = tempfile::tempdir().unwrap();
    let out = shell_dir.path().join("env.txt");
    let shell = recording_shell(shell_dir.path(), &out);
    KagiApp::set_terminal_shell_for_e2e(Some(shell.display().to_string()));

    let (app, window) = mount(cx, &repo);
    start_terminal(cx, &app, window);
    let seen = wait_for(&out);

    let stored = kagi_git::worktree_ports::Assignments::read()
        .port(&repo)
        .expect("starting the terminal stored a block for this worktree");
    let (port, path) = seen
        .strip_prefix("KAGI_PORT=")
        .and_then(|rest| rest.split_once("\nKAGI_WORKTREE_PATH="))
        .expect("the shell wrote both variables");
    assert_eq!(
        port,
        stored.to_string(),
        "the spawned shell received the worktree's stored block"
    );
    // Compared as a path: the value is git's workdir, which ends in `/`.
    assert_eq!(
        Path::new(path.trim_end()),
        repo.as_path(),
        "the spawned shell received this worktree's path"
    );

    // The sidebar shows the main worktree's block (assigned by the terminal
    // just now) and `pre`'s (read from the store by the snapshot); `side`
    // never had one, so it shows none.
    assert!(
        laid_out(cx, window, "sidebar-worktree-port-main"),
        "the main worktree's localhost:{stored} is shown"
    );
    assert!(
        laid_out(cx, window, "sidebar-worktree-port-pre"),
        "the pre-assigned worktree's localhost:{pre_port} is shown"
    );
    assert_eq!(
        kagi_git::worktree_ports::Assignments::read().port(&side),
        None,
        "reading the sidebar assigned nothing"
    );
    assert!(
        !laid_out(cx, window, "sidebar-worktree-port-side"),
        "a worktree with no stored block shows no link"
    );

    // A narrow sidebar never squeezes the name (#858 Tier B: at the default
    // ~220px the row read `✓… localhost:3000`). The path gives way first; a
    // link that still does not fit leaves the row whole (wrapped onto the
    // clipped second line), never cut down to part of its text.
    let layout = |cx: &mut VisualTestAppContext, sidebar: f32| {
        app.update(cx, |app, cx| {
            app.sidebar.width = sidebar;
            cx.notify();
        });
        let name = bounds(cx, window, "sidebar-worktree-name-main");
        let port = bounds(cx, window, "sidebar-worktree-port-main");
        let gap = f32::from(port.center().y) - f32::from(name.center().y);
        let same_line = gap.abs() < f32::from(name.size.height);
        (
            f32::from(name.size.width),
            f32::from(port.size.width),
            same_line,
        )
    };
    let (name_wide, port_wide, shown_wide) = layout(cx, 600.);
    assert!(
        name_wide > 0. && port_wide > 0.,
        "wide: name and link drawn"
    );
    assert!(shown_wide, "wide: the link sits on the row");
    for sidebar in [220., 140.] {
        let (name, port, _) = layout(cx, sidebar);
        assert_eq!(name, name_wide, "{sidebar}px: the name is not squeezed");
        assert_eq!(
            port, port_wide,
            "{sidebar}px: the link is whole or gone, never cut"
        );
    }
    let (_, _, shown_narrow) = layout(cx, 140.);
    assert!(
        !shown_narrow,
        "140px: with no room left, the link leaves the row"
    );

    KagiApp::set_terminal_shell_for_e2e(None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_port_env: the shell got KAGI_PORT={stored}");
}
