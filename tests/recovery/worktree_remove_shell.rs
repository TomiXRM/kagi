//! #867: removing a worktree while a Kagi terminal shell runs in it.
//!
//! Kagi never ends the user's processes. A running shell in the target blocks
//! the remove plan, also after its tab closed (the shell may ignore the
//! hangup); once it exits, the plan goes through. A process that outlived the
//! shell in its session (`nohup`) only warns.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_domain::plan_note::{PlanNote, WorktreeNote};

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_worktree_ports::{shell_live, start_terminal};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let script = dir.join(name);
    std::fs::write(&script, format!("#!/bin/sh\n{body}")).unwrap();
    std::process::Command::new("chmod")
        .args(["+x", script.to_str().unwrap()])
        .status()
        .unwrap();
    script
}

/// A shell that leaves a `nohup` job in its session (its PID in
/// `leftover.pid`), then waits on its terminal (exits at EOF). The scenario
/// kills the job by PID when done, so its length only has to outlast the
/// scenario: with `sleep 30` the leftover assertion failed in 2 of 4 runs of a
/// five-scenario filter (the cause was not pinned down); `sleep 600` passed
/// 8 of 8.
fn leaving_shell(dir: &Path) -> PathBuf {
    let pid = dir.join("leftover.pid");
    let body = format!(
        "nohup sleep 600 >/dev/null 2>&1 &\necho $! > '{}'\nexec cat\n",
        pid.display()
    );
    script(dir, "leave.sh", &body)
}

/// A shell that ignores the hangup closing its tab sends, and does not read
/// the terminal: it outlives the tab until killed by PID (in `shell.pid`).
pub(crate) fn hangup_proof_shell(dir: &Path) -> PathBuf {
    let pid = dir.join("shell.pid");
    let body = format!(
        "trap '' HUP\necho $$ > '{}'\nexec sleep 600\n",
        pid.display()
    );
    script(dir, "stay.sh", &body)
}

/// Kills whatever `leftover.pid` / `shell.pid` in `dir` name when dropped, so
/// a failed assertion does not leave the long-sleeping probes behind (#899
/// review). Killing an already-stopped PID is harmless.
pub(crate) struct KillRecordedOnDrop(pub(crate) PathBuf);

impl Drop for KillRecordedOnDrop {
    fn drop(&mut self) {
        for name in ["leftover.pid", "shell.pid"] {
            if let Ok(pid) = std::fs::read_to_string(self.0.join(name)) {
                let pid = pid.trim();
                if !pid.is_empty() {
                    let _ = std::process::Command::new("kill").arg(pid).status();
                }
            }
        }
    }
}

/// Stop the process this test started, by the PID it recorded — never by
/// name, which would reach a developer's own processes.
pub(crate) fn kill_recorded(pid_file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        if let Ok(pid) = std::fs::read_to_string(pid_file) {
            if !pid.trim().is_empty() {
                break pid.trim().to_string();
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} was never written",
            pid_file.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    std::process::Command::new("kill")
        .arg(&pid)
        .status()
        .unwrap();
}

fn wait_until(
    cx: &mut VisualTestAppContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestAppContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Plan removing `side` from the main tab; the notes of the plan shown.
fn plan_notes(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> (Vec<WorktreeNote>, Vec<WorktreeNote>) {
    app.update(cx, |app, cx| {
        app.cancel_remove_worktree_modal();
        app.open_remove_worktree_modal("side".into(), false, cx);
    });
    wait_until(cx, "the remove plan never arrived", |cx| {
        cx.read(|cx| app.read(cx).remove_worktree_modal().is_some())
    });
    shown_notes(cx, app)
}

fn shown_notes(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> (Vec<WorktreeNote>, Vec<WorktreeNote>) {
    let worktree = |notes: &[PlanNote]| {
        notes
            .iter()
            .filter_map(|note| match note {
                PlanNote::Worktree(note) => Some(note.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    cx.read(|cx| {
        let plan = &app.read(cx).remove_worktree_modal().unwrap().plan;
        (worktree(&plan.blockers), worktree(&plan.warnings))
    })
}

fn live_blocker(blockers: &[WorktreeNote]) -> bool {
    blockers
        .iter()
        .any(|note| matches!(note, WorktreeNote::RemoveLiveShell { .. }))
}

fn leftover(warnings: &[WorktreeNote]) -> Option<usize> {
    warnings.iter().find_map(|note| match note {
        WorktreeNote::RemoveLeftoverProcesses { count, .. } => Some(*count),
        _ => None,
    })
}

fn confirm(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    app.update(cx, |app, cx| app.confirm_remove_worktree(cx));
    wait_until(cx, "the remove never settled", |cx| {
        cx.read(|cx| app.read(cx).write_busy_op.is_none())
    });
}

fn switch(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, tab: usize) {
    app.update(cx, |app, cx| app.switch_repo(tab, cx));
    cx.run_until_parked();
}

pub fn scenario_worktree_remove_live_shell(cx: &mut VisualTestAppContext) {
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
    let _probes = KillRecordedOnDrop(shell_dir.path().to_path_buf());
    KagiApp::set_terminal_shell_for_e2e(Some(
        leaving_shell(shell_dir.path()).display().to_string(),
    ));

    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    assert!(app.update(cx, |app, cx| app.open_repository(side.clone(), cx)));
    cx.run_until_parked();
    let side_tab = cx.read(|cx| app.read(cx).tabs.len() - 1);
    switch(cx, &app, 0);

    // No shell yet: a clean plan.
    let (blockers, _) = plan_notes(cx, &app);
    assert!(blockers.is_empty(), "no shell, no blocker: {blockers:?}");

    // A shell runs in the target: blocked, and confirming refuses.
    switch(cx, &app, side_tab);
    start_terminal(cx, &app, window);
    assert!(shell_live(cx, &app), "the side worktree's shell starts");
    switch(cx, &app, 0);
    let (blockers, _) = plan_notes(cx, &app);
    assert!(
        live_blocker(&blockers),
        "a running shell blocks: {blockers:?}"
    );
    let reason = cx.read(|cx| {
        let modal = app.read(cx).remove_worktree_modal().unwrap();
        kagi::ui::i18n::plan_note_text(&modal.plan.blockers[0])
    });
    assert!(
        reason.contains("exit") && reason.contains(side.to_str().unwrap()),
        "the reason says to exit, and where: {reason}"
    );
    confirm(cx, &app);
    assert!(side.exists(), "a blocked plan removes nothing");
    let notice = cx.read(|cx| {
        kagi::ui::e2e::app_notice_message(app.read(cx))
            .unwrap_or_default()
            .to_string()
    });
    assert!(notice.contains(&reason), "the refusal says why: {notice}");
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();

    // The shell exits; its `nohup` job is still in its session: no blocker,
    // a warning.
    switch(cx, &app, side_tab);
    cx.read(|cx| {
        let session = app.read(cx).ui().terminal_session.as_ref().unwrap();
        session.paste_writer.as_ref().unwrap().paste_text("\u{4}");
    });
    wait_until(cx, "the side shell never exited", |cx| {
        !shell_live(cx, &app)
    });
    switch(cx, &app, 0);
    let (blockers, warnings) = plan_notes(cx, &app);
    assert!(
        blockers.is_empty(),
        "an exited shell never blocks: {blockers:?}"
    );
    assert_eq!(
        leftover(&warnings),
        Some(1),
        "the job the shell left is reported: {warnings:?}"
    );

    // With the job gone too, nothing is left to report.
    kill_recorded(&shell_dir.path().join("leftover.pid"));
    let deadline = Instant::now() + Duration::from_secs(5);
    let (blockers, warnings) = loop {
        let notes = plan_notes(cx, &app);
        if leftover(&notes.1).is_none() || Instant::now() > deadline {
            break notes;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        blockers.is_empty() && leftover(&warnings).is_none(),
        "{warnings:?}"
    );

    // #867 review: closing the tab does not end a shell that ignores the
    // hangup. It still runs in the worktree, so it still blocks, until it
    // exits.
    KagiApp::set_terminal_shell_for_e2e(Some(
        hangup_proof_shell(shell_dir.path()).display().to_string(),
    ));
    switch(cx, &app, side_tab);
    start_terminal(cx, &app, window);
    assert!(shell_live(cx, &app), "the second side shell starts");
    switch(cx, &app, 0);
    app.update(cx, |app, cx| app.close_tab(side_tab, cx));
    cx.run_until_parked();
    let (blockers, _) = plan_notes(cx, &app);
    assert!(
        live_blocker(&blockers),
        "a closed tab's shell that is still running blocks: {blockers:?}"
    );
    kill_recorded(&shell_dir.path().join("shell.pid"));
    let deadline = Instant::now() + Duration::from_secs(5);
    let blockers = loop {
        let (blockers, _) = plan_notes(cx, &app);
        if blockers.is_empty() || Instant::now() > deadline {
            break blockers;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        blockers.is_empty(),
        "its exit lifts the block: {blockers:?}"
    );
    confirm(cx, &app);
    assert!(
        !side.exists(),
        "with the shell gone, the worktree is removed"
    );

    KagiApp::set_terminal_shell_for_e2e(None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_remove_live_shell");
}
