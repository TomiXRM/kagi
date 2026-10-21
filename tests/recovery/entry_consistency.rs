//! #1697: explicit safe intent is consistent across real UI entry routes.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{commands, e2e, i18n, KagiApp};
use kagi_git::oplog::OpOutcome;

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::pull_freshness::{clean_updates_from, Checkout, Origin, SettingsGuard};
use crate::recovery_operations::wait_idle;

#[path = "../support/git_fixture.rs"]
mod git_fixture;
#[path = "../support/oplog.rs"]
mod oplog;
use git_fixture::{git, git_output as output};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn configured_shortcut(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    command: &'static str,
) {
    draw(cx, window);
    let binding = commands::effective_keystroke(command).expect("configured operation shortcut");
    // The registry uses the platform-neutral secondary modifier; native macOS
    // key events carry Command. Do not dispatch an action directly here.
    cx.simulate_keystrokes(window, &binding.replace("secondary", "cmd"));
}

fn palette(cx: &mut VisualTestAppContext, window: AnyWindowHandle, query: &str) {
    configured_shortcut(cx, window, "view.commandPalette");
    cx.run_until_parked();
    draw(cx, window);
    cx.simulate_keystrokes(window, query);
    cx.run_until_parked();
    draw(cx, window);
    cx.simulate_keystrokes(window, "enter");
}

fn palette_pull(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    palette(cx, window, "p u l l");
}

fn shortcut_pull(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    configured_shortcut(cx, window, "repo.pull");
}

fn palette_push(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    palette(cx, window, "p u s h");
}

fn shortcut_push(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    configured_shortcut(cx, window, "repo.push");
}

fn branch_push(cx: &mut VisualTestAppContext, window: AnyWindowHandle, publish: bool) {
    draw(cx, window);
    let row = e2e::control_bounds(window.window_id(), "sidebar-local-feature")
        .expect("noncurrent branch row is drawn");
    cx.simulate_mouse_down(
        window,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        window,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    draw(cx, window);
    let advanced = e2e::control_bounds(window.window_id(), "branch-menu-item-3-2")
        .expect("Advanced submenu is drawn");
    cx.simulate_click(window, advanced.center(), gpui::Modifiers::none());
    draw(cx, window);
    let id = if publish {
        "branch-menu-item-3-2-submenu-3-2"
    } else {
        "branch-menu-item-3-2-submenu-2-1"
    };
    let action = e2e::control_bounds(window.window_id(), id)
        .expect("actual sidebar Push/Publish action is drawn");
    cx.simulate_click(window, action.center(), gpui::Modifiers::none());
}

fn assert_success_receipt(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    let durable = oplog::records(repo, "push", 100, true);
    assert_eq!(durable.len(), 1, "one intent produces one persisted Push");
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!e2e::active_modal_present(app));
        assert!(!app.app_sessions.has_leases());
        let panel = app.op_log.as_ref().expect("Operation Log").read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "push" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, durable[0].id, "visible receipt is durable");
    });
}

#[derive(Clone, Copy)]
enum PushEntry {
    SidebarPush,
    SidebarPublish,
    Palette,
    Shortcut,
}

fn push_from(cx: &mut VisualTestAppContext, entry: PushEntry) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let _origin = Origin::new(&repo);
    let sidebar = matches!(entry, PushEntry::SidebarPush | PushEntry::SidebarPublish);
    let publish = matches!(entry, PushEntry::SidebarPublish);
    let branch = if sidebar { "feature" } else { "main" };
    if sidebar {
        git(&repo, &["branch", branch]);
        if !publish {
            git(&repo, &["push", "-q", "-u", "origin", branch]);
        }
    }
    git(
        &repo,
        &["commit", "-qm", "safe outgoing intent", "--allow-empty"],
    );
    if sidebar {
        git(&repo, &["branch", "-f", branch, "HEAD"]);
    }
    std::fs::write(repo.join("README.md"), "staged local content\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["update-index", "--chmod=+x", "README.md"]);
    std::fs::write(repo.join("README.md"), "unstaged local content\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "retained local content\n").unwrap();
    let before = Checkout::read(&repo);
    let target = format!("refs/heads/{branch}");
    let outgoing = output(&repo, &["rev-parse", &target]);
    let remote_before = output(&repo, &["ls-remote", "origin", &target]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    match entry {
        PushEntry::SidebarPush => branch_push(cx, window, false),
        PushEntry::SidebarPublish => branch_push(cx, window, true),
        PushEntry::Palette => palette_push(cx, window),
        PushEntry::Shortcut => shortcut_push(cx, window),
    }
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.app_sessions.has_leases(), "real route admits the write");
        assert!(
            !e2e::active_modal_present(app),
            "safe intent needs no extra modal"
        );
    });
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(
        output(&repo, &["ls-remote", "origin", &target]),
        remote_before
    );
    assert!(oplog::records(&repo, "push", 100, true).is_empty());
    release.send(());
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(
        Checkout::read(&repo),
        before,
        "Push never changes HEAD/index/worktree"
    );
    assert_eq!(
        output(&repo, &["ls-remote", "origin", &target]),
        format!("{outgoing}\t{target}")
    );
    if publish {
        assert_eq!(
            output(&repo, &["config", "branch.feature.remote"]),
            "origin"
        );
        assert_eq!(output(&repo, &["config", "branch.feature.merge"]), target);
    }
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
    assert_success_receipt(cx, &app, &repo);
    unmount(cx, app, window);
}

pub fn scenario_sidebar_push_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    push_from(cx, PushEntry::SidebarPush);
    eprintln!("[gui-e2e] PASS entry_consistency_sidebar_push_safe_intent");
}

pub fn scenario_sidebar_publish_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    push_from(cx, PushEntry::SidebarPublish);
    eprintln!("[gui-e2e] PASS entry_consistency_sidebar_publish_safe_intent");
}

pub fn scenario_palette_pull_push_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    i18n::set_lang(i18n::Lang::En);
    clean_updates_from(cx, palette_pull);
    push_from(cx, PushEntry::Palette);
    eprintln!("[gui-e2e] PASS entry_consistency_palette_pull_push_safe_intent");
}

pub fn scenario_shortcut_pull_push_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    i18n::set_lang(i18n::Lang::En);
    clean_updates_from(cx, shortcut_pull);
    push_from(cx, PushEntry::Shortcut);
    eprintln!("[gui-e2e] PASS entry_consistency_shortcut_pull_push_safe_intent");
}

pub fn scenario_sidebar_noncurrent_pull_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        crate::pull_freshness::branch_ref_updates(cx, false, false, "origin");
    }
    eprintln!("[gui-e2e] PASS entry_consistency_sidebar_noncurrent_pull_safe_intent");
}
