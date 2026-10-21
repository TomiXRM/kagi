//! #1699: branch/menu targets describe observed state, not a guessed operation.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::native_ax;
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{point, px, VisualTestAppContext};
use kagi::ui::{dialog_a11y, e2e};
use kagi_domain::branch_cleanup::{CleanupDeleteTarget, MergedBranchStatus};
use kagi_ui_core::i18n::{self, Msg};
use std::path::Path;

fn output(repo: &Path, args: &[&str]) -> String {
    let result = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

pub fn branch_menu_merge_labels(cx: &mut VisualTestAppContext) {
    let _settings = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged", "HEAD~1"]);
    for branch in ["unmerged", "squashed"] {
        git(repo, &["checkout", "-q", "-b", branch, "main"]);
        std::fs::write(repo.join(branch), branch).unwrap();
        git(repo, &["add", branch]);
        git(repo, &["commit", "-q", "-m", branch]);
    }
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["merge", "--squash", "squashed"]);
    git(repo, &["commit", "-q", "-m", "squash both changes"]);
    // A genuinely unmerged tip after the squash, not a squash-proven ancestor.
    git(repo, &["checkout", "-q", "unmerged"]);
    std::fs::write(repo.join("unmerged-later"), "new work").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "unmerged later"]);
    git(repo, &["checkout", "-q", "main"]);
    let before = output(repo, &["show-ref"]);
    let tree = output(repo, &["rev-parse", "unmerged^{tree}"]);
    let object = repo.join(".git/objects").join(&tree[..2]).join(&tree[2..]);
    let held = object.with_extension("held");
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        let (app, window) = mount(cx, repo);
        native_ax::activate(cx);
        for (branch, merged) in [
            ("merged", Some(true)),
            ("squashed", Some(true)),
            ("unmerged", Some(false)),
            ("unmerged", None),
        ] {
            if merged.is_none() {
                std::fs::rename(&object, &held).unwrap();
            }
            app.update(cx, |app, cx| {
                app.open_local_branch_menu(branch.into(), point(px(400.), px(100.)));
                assert_eq!(app.branch_menu.as_ref().unwrap().merged_into_current, None);
                cx.notify();
            });
            draw(cx, window, (1440., 900.));
            cx.run_until_parked();
            draw(cx, window, (1440., 900.));
            assert_eq!(
                app.read_with(cx, |app, _| app
                    .branch_menu
                    .as_ref()
                    .unwrap()
                    .merged_into_current),
                merged
            );
            let node = native_ax::node(cx, window, "branch-menu-item-2-2".into());
            assert_eq!(
                node["aria"]["label"],
                if merged == Some(false) {
                    Msg::BcmDeleteUnmerged.t()
                } else {
                    Msg::BcmDeleteBranch.t()
                }
            );
            crate::keyboard_nav::keys(cx, window, "escape");
            assert!(app.read_with(cx, |app, _| app.branch_menu.is_none()));
            if merged.is_none() {
                std::fs::rename(&held, &object).unwrap();
            }
        }
        unmount(cx, app, window);
    }
    assert_eq!(output(repo, &["show-ref"]), before);
    assert!(output(repo, &["status", "--porcelain"]).is_empty());
}

pub fn branch_cleanup_target_heading(cx: &mut VisualTestAppContext) {
    let _settings = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged", "HEAD~1"]);
    let tip = kagi_git::CommitId(output(repo, &["rev-parse", "merged"]));
    let before = output(repo, &["show-ref"]);
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        let (app, window) = mount(cx, repo);
        dialog_a11y::clear_recorded_a11y();
        app.update(cx, |app, cx| {
            app.open_branch_cleanup_plan(
                vec![CleanupDeleteTarget {
                    name: "merged".into(),
                    local_tip: Some(tip.clone()),
                    remote_tip: None,
                    status: MergedBranchStatus::FullyMerged,
                }],
                cx,
            );
            assert!(app.branch_cleanup_modal().unwrap().plan.blockers.is_empty());
        });
        draw(cx, window, (1440., 900.));
        assert_eq!(
            dialog_a11y::recorded_note("plan-preview"),
            Some((
                gpui::Role::Group,
                format!("{}: 1", Msg::PlanBranchesToDelete.t())
            ))
        );
        assert!(e2e::control_bounds(window.window_id(), "modal-target-list").is_some());
        crate::keyboard_nav::keys(cx, window, "escape");
        assert!(app.read_with(cx, |app, _| app.branch_cleanup_modal().is_none()));
        assert!(!app.read_with(cx, |app, _| app.app_sessions.has_leases()));
        unmount(cx, app, window);
    }
    assert_eq!(output(repo, &["show-ref"]), before);
    assert!(output(repo, &["status", "--porcelain"]).is_empty());
}
