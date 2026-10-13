//! #1442: restore the active document identity after filtering saved tabs.
use gpui::VisualTestAppContext;
use kagi::ui::{e2e, settings, tabs, KagiApp, RemoteRepoView};

fn restore() -> KagiApp {
    let mut app = KagiApp::with_error("");
    tabs::restore_saved_session(&mut app);
    app
}

pub fn scenario_tab_restore_active_identity(_: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["session_repos", "session_active"]);
    let a = crate::macos::build_fixture();
    let ssh = crate::macos::build_fixture();
    let b = crate::macos::build_fixture();
    let c = crate::macos::build_fixture();
    let mut app = e2e::app_state(a.path()).unwrap();
    let mut remote = e2e::app_state(ssh.path()).unwrap().tabs.remove(0);
    // Even a valid local path must not be persisted for an SSH descriptor.
    remote.remote = Some(RemoteRepoView {
        host: kagi_domain::remote::RemoteHost::parse("audit-host").unwrap(),
        root: ssh.path().display().to_string(),
    });
    app.tabs.push(remote);
    app.tabs
        .push(e2e::app_state(b.path()).unwrap().tabs.remove(0));
    app.tabs
        .push(e2e::app_state(c.path()).unwrap().tabs.remove(0));
    app.active_tab = 2;
    app.repo_path = Some(b.path().to_path_buf());
    app.save_session();
    let restored = restore();
    assert_eq!(
        restored.tabs.len(),
        3,
        "SSH is not restored as a local repository"
    );
    assert_eq!(
        restored.repo_path.as_deref(),
        Some(b.path()),
        "[A, SSH, B, C] with B active must restore B, not C"
    );
    let saved = settings::read_setting("session_repos").unwrap();
    assert!(!saved
        .split('\u{1f}')
        .any(|path| path == ssh.path().to_str().unwrap()));
    // The active identity survives an unopenable predecessor, not just an SSH exclusion.
    std::fs::rename(a.path().join(".git"), a.path().join("not-git")).unwrap();
    assert_eq!(
        restore().repo_path.as_deref(),
        Some(b.path()),
        "an unopenable predecessor must not shift the active document"
    );
    std::fs::rename(b.path().join(".git"), b.path().join("not-git")).unwrap();
    assert_eq!(
        restore().repo_path.as_deref(),
        Some(c.path()),
        "a missing active identity falls back to the first restored repository"
    );
    std::fs::rename(c.path().join(".git"), c.path().join("not-git")).unwrap();
    let empty = restore();
    assert!(
        empty.tabs.is_empty() && empty.repo_path.is_none() && empty.home_in_front(),
        "all missing repositories fall back to initial Home"
    );
    // Legacy numeric settings resolve in the saved set before any open failures.
    std::fs::rename(b.path().join("not-git"), b.path().join(".git")).unwrap();
    std::fs::rename(c.path().join("not-git"), c.path().join(".git")).unwrap();
    settings::write_setting("session_active", Some("1"));
    assert_eq!(restore().repo_path.as_deref(), Some(b.path()));
    app.active_tab = 1;
    app.repo_path = None;
    app.save_session();
    assert_eq!(
        restore().repo_path.as_deref(),
        Some(b.path()),
        "an ephemeral active SSH tab falls back to the first surviving local tab"
    );
}
