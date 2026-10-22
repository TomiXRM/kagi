//! Actual SSH command adapter + Git writes + remote snapshot must keep one result.
use crate::busy_dismiss::{paint, wait};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{e2e, i18n::Lang, FooterStatus, ToastKind};
use kagi_git::oplog::{self, OpOutcome};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

pub fn scenario_remote_stash_result_survives_refresh(cx: &mut VisualTestAppContext) {
    scenarios(cx, false);
    eprintln!("[gui-e2e] PASS remote_stash_result_survives_refresh");
}
pub fn scenario_remote_pull_result_survives_refresh(cx: &mut VisualTestAppContext) {
    scenarios(cx, true);
    eprintln!("[gui-e2e] PASS remote_pull_result_survives_refresh");
}

fn scenarios(cx: &mut VisualTestAppContext, pull: bool) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "recent_repos"]);
    for lang in [Lang::En, Lang::Ja] {
        for active in [true, false] {
            completed_refresh(cx, pull, lang, active);
        }
    }
}

fn completed_refresh(cx: &mut VisualTestAppContext, pull: bool, lang: Lang, active: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::fetch_owner::local_remote_with_change(&repo, remote.path());
    git(&repo, &["fetch", "-q", "origin"]);
    for text in ["one\n", "two\n", "three\n"] {
        std::fs::write(repo.join("README.md"), text).unwrap();
        git(&repo, &["stash", "push", "-qm", text.trim()]);
    }
    let before_head = git_output(&repo, &["rev-parse", "HEAD"]);
    let upstream = git_output(&repo, &["rev-parse", "origin/main"]);
    let before_stashes = git_output(&repo, &["stash", "list", "--format=%H"]);
    let snapshot = kagi_git::Backend::open(&repo).unwrap().snapshot(10_000).unwrap();
    let shim = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    // Keep the real explicit-identity prerequisite; no user key is read.
    let identity = shim.path().join("identity");
    assert!(std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "owned-kagi-fixture", "-f"])
        .arg(&identity).status().unwrap().success());
    let script = shim.path().join("ssh");
    // This replaces only SSH transport: execute the unchanged production
    // protocol shell locally against this disposable repository/runtime. Both
    // the write and the following remote_snapshot run their real Git commands.
    std::fs::write(&script, format!(
        "#!/bin/sh\nif [ \"$1\" = -G ]; then\n printf 'hostname e2e.invalid\\nuser kagi-e2e\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\nproxyjump none\\n'\n exit 0\nfi\nfor argument do command=$argument; done\nexport HOME='{}'\nexport XDG_RUNTIME_DIR='{}'\nexec /bin/sh -c \"$command\"\n",
        repo.display(), runtime.path().display()
    )).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", format!("{}:{}", shim.path().display(), old_path.clone().unwrap_or_default().to_string_lossy()));
    let host = kagi_domain::remote::RemoteHost {
        user: Some("kagi-e2e".into()), host: "e2e.invalid".into(),
        port: None, identity_file: Some(identity.to_str().unwrap().into()),
    };
    let root = repo.to_str().unwrap().to_owned();
    let recorded_repo = format!("{}:{root}", host.label());
    let op = if pull { "pull" } else { "stash-drop" };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_window_active_for_e2e(active, cx);
        app.enter_remote_view(host.clone(), root.clone(), snapshot, cx);
        if pull { app.open_pull_modal(cx); }
        else { app.open_stash_drop_modal(1, cx); }
    });
    wait(cx, &app, "the real SSH identity plan is confirmable", |app| {
        matches!(app.app_sessions.plan_state(), kagi::app::PlanState::Ready { .. })
    });
    let (owner, tab) = cx.read(|cx| {
        let state = app.read(cx);
        (state.active_session().unwrap(), state.active_tab)
    });
    paint(cx, &app, window);
    let confirm = e2e::confirm_bounds(window.window_id()).expect("real remote confirmation");
    cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
    wait(cx, &app, "write and actual SSH snapshot both finish", |app| {
        !app.app_sessions.has_leases()
            && app.active_session().is_some_and(|session| session != owner)
            && app.remote_view.as_ref().is_some_and(|view| view.host == host && view.root == root)
    });
    paint(cx, &app, window);
    let entries = oplog::read_oplog_tail(usize::MAX);
    let receipts: Vec<_> = entries.iter()
        .filter(|entry| entry.op == op && entry.repo == recorded_repo).collect();
    assert_eq!(receipts.len(), 1);
    assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
    if pull {
        assert_ne!(before_head, upstream);
        assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), upstream);
        assert_eq!(git_output(&repo, &["stash", "list", "--format=%H"]), before_stashes);
    } else {
        assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
        let retained: Vec<_> = before_stashes.lines().enumerate()
            .filter(|(index, _)| *index != 1).map(|(_, oid)| oid).collect();
        assert_eq!(git_output(&repo, &["stash", "list", "--format=%H"]).lines().collect::<Vec<_>>(), retained);
    }
    assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
    cx.read(|cx| {
        let state = app.read(cx);
        let stack = state.toast_stack.as_ref().unwrap().read(cx);
        eprintln!("[gui-e2e] remote_result_refresh op={op} lang={lang:?} active={active} original_session={owner:?} refreshed_session={:?} footer={:?} toasts={:?}", state.active_session(), state.status_footer, stack.toasts().iter().map(|toast| (toast.id, toast.kind, &toast.action)).collect::<Vec<_>>());
        assert_eq!(state.active_tab, tab, "automatic refresh must keep the same view");
        assert!(!matches!(state.status_footer, FooterStatus::Busy(_)));
        assert_eq!(stack.toasts().len(), 1, "the request's result survives its real refresh");
        assert_eq!(stack.toasts()[0].kind, ToastKind::Success);
        let panel = state.op_log.as_ref().unwrap().read(cx);
        assert_eq!(panel.entries().iter().filter(|entry| entry.id == receipts[0].id
            && entry.repo == receipts[0].repo && entry.op == receipts[0].op).count(), 1);
        assert!(state.app_notice().is_none());
    });
    unmount(cx, app, window);
    match old_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
}
