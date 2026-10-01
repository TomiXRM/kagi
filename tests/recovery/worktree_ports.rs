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

pub(crate) fn start_terminal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

pub fn scenario_worktree_port_env(cx: &mut VisualTestAppContext) {
    let _ports = crate::gui_isolation::PortStore::keep();
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
    let _probes =
        crate::recovery_worktree_remove_shell::KillRecordedOnDrop(shell_dir.path().to_path_buf());
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
    // #870: exactly the worktree's path — no trailing `/` from git's workdir.
    assert_eq!(
        path.trim_end_matches('\n'),
        repo.display().to_string(),
        "the spawned shell received this worktree's path"
    );

    // Main still owns its stored block but has no WORKTREES leaf. `pre` has
    // its own pre-assigned block; `side` never had one.
    assert!(
        !laid_out(cx, window, "sidebar-worktree-port-main"),
        "main worktree should not have a sidebar row"
    );
    assert!(
        laid_out(cx, window, "sidebar-worktree-port-pre"),
        "the pre-assigned linked worktree's localhost:{pre_port} is shown"
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
        let name = bounds(cx, window, "sidebar-worktree-name-pre");
        let port = bounds(cx, window, "sidebar-worktree-port-pre");
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

/// A shell that only waits on its terminal: it exits when it reads EOF.
fn waiting_shell(dir: &Path) -> PathBuf {
    let script = dir.join("wait.sh");
    std::fs::write(&script, "#!/bin/sh\nexec cat\n").unwrap();
    std::process::Command::new("chmod")
        .args(["+x", script.to_str().unwrap()])
        .status()
        .unwrap();
    script
}

/// Whether the active tab's terminal has a shell Kagi has not seen exit.
pub(crate) fn shell_live(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> bool {
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .and_then(|t| t.shell.as_ref())
            .is_some_and(|shell| shell.exit.is_none())
    })
}

fn footer(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> String {
    cx.read(|cx| match &app.read(cx).status_footer {
        kagi::ui::FooterStatus::Failed(text) => text.to_string(),
        _ => String::new(),
    })
}

/// #859 / ADR-0213: with `worktree_run_mode` = `nonconcurrent`, while one
/// worktree of a repository has a running terminal shell, another worktree of
/// it starts none — the footer says which one is running — and it starts once
/// that shell exits. The default (`concurrent`) starts both.
pub fn scenario_worktree_nonconcurrent(cx: &mut VisualTestAppContext) {
    let _ports = crate::gui_isolation::PortStore::keep();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let side_dir = tempfile::tempdir().unwrap();
    let side = side_dir.path().canonicalize().unwrap().join("side");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            side.to_str().unwrap(),
        ],
    );
    let shell_dir = tempfile::tempdir().unwrap();
    let _probes =
        crate::recovery_worktree_remove_shell::KillRecordedOnDrop(shell_dir.path().to_path_buf());
    KagiApp::set_terminal_shell_for_e2e(Some(
        waiting_shell(shell_dir.path()).display().to_string(),
    ));
    kagi::ui::settings::write_setting("worktree_run_mode", Some("nonconcurrent"));

    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    assert!(app.update(cx, |app, cx| app.open_repository(side.clone(), cx)));
    cx.run_until_parked();
    let side_tab = cx.read(|cx| app.read(cx).tabs.len() - 1);

    // The linked worktree's shell runs.
    app.update(cx, |app, cx| app.switch_repo(side_tab, cx));
    cx.run_until_parked();
    start_terminal(cx, &app, window);
    assert!(shell_live(cx, &app), "the first worktree's shell starts");
    // #869: the linked worktree's link is the main worktree's block, which
    // its shell was handed; main is not rendered as a sidebar leaf.
    draw(cx, window);
    let row_ports = cx.read(|cx| {
        app.read(cx)
            .sidebar
            .rows
            .iter()
            .filter_map(|row| match row {
                kagi::ui::sidebar::SidebarRow::Worktree { name, port, .. } => {
                    Some((name.clone(), *port))
                }
                _ => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    });
    let stored = kagi_git::worktree_ports::Assignments::read();
    assert!(
        row_ports["side"].is_some()
            && row_ports["side"] == stored.port(&repo)
            && !row_ports.contains_key("main")
            && stored.port(&side).is_none(),
        "nonconcurrent: the linked row uses the hidden main block: {row_ports:?}"
    );

    // The main worktree of the same repository is refused while it does.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    start_terminal(cx, &app, window);
    assert!(
        !shell_live(cx, &app),
        "a second worktree's shell must not start while the first runs"
    );
    let refused = footer(cx, &app);
    assert!(
        refused.contains("nonconcurrent") && refused.contains(side.to_str().unwrap()),
        "the footer names the running worktree: {refused}"
    );

    // The first shell exits (EOF to `cat`): the main worktree may start now.
    app.update(cx, |app, cx| app.switch_repo(side_tab, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let session = app.read(cx).ui().terminal_session.as_ref().unwrap();
        session.paste_writer.as_ref().unwrap().paste_text("\u{4}");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while shell_live(cx, &app) {
        assert!(
            std::time::Instant::now() < deadline,
            "the first shell never exited"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    start_terminal(cx, &app, window);
    assert!(
        shell_live(cx, &app),
        "with the other shell gone, this worktree's starts"
    );

    // #877 review: a tab closed while its shell ignores the hangup. The
    // shell still runs in the side worktree, so the main worktree is still
    // refused, until it exits.
    cx.read(|cx| {
        let session = app.read(cx).ui().terminal_session.as_ref().unwrap();
        session.paste_writer.as_ref().unwrap().paste_text("\u{4}");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while shell_live(cx, &app) {
        assert!(
            std::time::Instant::now() < deadline,
            "the main shell never exited"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    KagiApp::set_terminal_shell_for_e2e(Some(
        crate::recovery_worktree_remove_shell::hangup_proof_shell(shell_dir.path())
            .display()
            .to_string(),
    ));
    app.update(cx, |app, cx| app.switch_repo(side_tab, cx));
    cx.run_until_parked();
    start_terminal(cx, &app, window);
    assert!(
        shell_live(cx, &app),
        "the side shell that ignores hangups starts"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.close_tab(side_tab, cx));
    cx.run_until_parked();
    // Shells from here on exit at unmount.
    KagiApp::set_terminal_shell_for_e2e(Some(
        waiting_shell(shell_dir.path()).display().to_string(),
    ));
    start_terminal(cx, &app, window);
    assert!(
        !shell_live(cx, &app),
        "a closed tab's shell that is still running keeps blocking"
    );
    crate::recovery_worktree_remove_shell::kill_recorded(&shell_dir.path().join("shell.pid"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !shell_live(cx, &app) {
        assert!(
            std::time::Instant::now() < deadline,
            "the main shell never started after the closed tab's shell exited"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(50));
        start_terminal(cx, &app, window);
    }
    assert!(app.update(cx, |app, cx| app.open_repository(side.clone(), cx)));
    cx.run_until_parked();
    let side_tab = cx.read(|cx| app.read(cx).tabs.len() - 1);

    // Concurrent (the default): the linked worktree starts alongside.
    kagi::ui::settings::write_setting("worktree_run_mode", None);
    app.update(cx, |app, cx| app.switch_repo(side_tab, cx));
    cx.run_until_parked();
    start_terminal(cx, &app, window);
    assert!(
        shell_live(cx, &app),
        "concurrent mode starts a second worktree's shell"
    );

    KagiApp::set_terminal_shell_for_e2e(None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_nonconcurrent");
}
