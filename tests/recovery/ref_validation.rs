//! #1379 / #1380: unexecutable refs cannot reach confirmation.
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_git::CommitId;
use kagi_ui_core::i18n::{self, Lang};
use std::time::Duration;
#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    // sync_modal_inputs runs on paint, then its 250ms replan timer must fire.
    cx.advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn type_name(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    input: &Entity<gpui_component::input::InputState>,
    requested: &str,
) {
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    let keys = requested
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    cx.simulate_keystrokes(window, &format!("cmd-a {keys}"));
    paint(cx, window);
}

fn ref_plan(app: &KagiApp, tag: bool) -> &kagi_git::OperationPlan {
    if tag {
        app.create_tag_modal().unwrap().plan.plan().unwrap()
    } else {
        app.create_branch_modal().unwrap().plan.plan().unwrap()
    }
}

pub fn scenario_create_ref_collision_blockers(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for tag in [false, true] {
            for packed in [false, true] {
                for (existing, requested) in [
                    ("feature", "feature"),
                    ("feature", "feature/x"),
                    ("feature/x", "feature"),
                ] {
                    create_ref_case(cx, tag, packed, existing, requested, true, false);
                }
                create_ref_case(cx, tag, packed, "feature/x", "feature/y", false, false);
                create_ref_case(cx, tag, packed, "feature", "feature", false, true);
            }
        }
    }
    eprintln!("[gui-e2e] PASS create_ref_collision_blockers");
}

fn create_ref_case(
    cx: &mut VisualTestAppContext,
    tag: bool,
    packed: bool,
    existing: &str,
    requested: &str,
    blocked: bool,
    other_namespace: bool,
) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(
        repo,
        &[
            if tag != other_namespace {
                "tag"
            } else {
                "branch"
            },
            existing,
        ],
    );
    if packed {
        git(repo, &["pack-refs", "--all"]);
    }
    let refs_before = git_fixture::git_output(repo, &["show-ref"]);
    let before = repo_fingerprint(repo);
    let head = CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        if tag {
            app.open_create_tag_modal(head, cx);
        } else {
            app.open_create_branch_modal(head, cx);
        }
    });
    paint(cx, window);
    let input = cx.read(|cx| {
        let app = app.read(cx);
        if tag {
            app.create_tag_modal().unwrap().input_state.clone().unwrap()
        } else {
            app.create_branch_modal()
                .unwrap()
                .input_state
                .clone()
                .unwrap()
        }
    });
    type_name(cx, window, &input, requested);
    cx.read(|cx| {
        let app = app.read(cx);
        let plan = ref_plan(app, tag);
        assert_eq!(
            !plan.blockers.is_empty(),
            blocked,
            "Confirm enabled for {requested} colliding with {existing}; tag={tag}, packed={packed}"
        );
        if blocked {
            let reason = i18n::plan_note_text(&plan.blockers[0]);
            assert!(
                reason.contains(&if existing == requested {
                    existing.to_string()
                } else {
                    format!("refs/{}/{existing}", if tag { "tags" } else { "heads" })
                }),
                "blocking ref must be named: {reason}"
            );
        }
    });
    if blocked {
        assert_blocked_create(cx, &app, window, tag, repo);
    }
    assert_eq!(git_fixture::git_output(repo, &["show-ref"]), refs_before);
    assert_eq!(repo_fingerprint(repo), before);
    drop(input);
    unmount(cx, app, window);
}

fn assert_blocked_create(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    tag: bool,
    repo: &std::path::Path,
) {
    let bounds = e2e::confirm_bounds(window.window_id()).expect("visible Create");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    paint(cx, window);
    // The actual field's Enter may record Refused, but never an execution receipt.
    cx.simulate_keystrokes(window, "enter");
    paint(cx, window);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.app_sessions.has_leases());
        assert!(if tag {
            app.create_tag_modal().is_some()
        } else {
            app.create_branch_modal().is_some()
        });
    });
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|e| e.op == if tag { "create-tag" } else { "create-branch" })
        .all(|e| matches!(e.outcome, kagi_git::oplog::OpOutcome::Refused { .. })));
}

pub fn scenario_set_upstream_missing_blocker(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        upstream_case(cx);
    }
    eprintln!("[gui-e2e] PASS set_upstream_missing_blocker");
}

fn upstream_case(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    git(repo, &["branch", "local-upstream"]);
    let config_before = std::fs::read(repo.join(".git/config")).unwrap();
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_set_upstream_modal("main".into());
        cx.notify();
    });
    paint(cx, window);
    let input = cx.read(|cx| {
        app.read(cx)
            .set_upstream_modal()
            .unwrap()
            .input_state
            .clone()
            .unwrap()
    });
    for requested in ["origin/missing", "other/main", "bad..name"] {
        type_name(cx, window, &input, requested);
        cx.read(|cx| {
            let app = app.read(cx);
            let plan = app.set_upstream_modal().unwrap().plan.plan().unwrap();
            assert!(
                !plan.blockers.is_empty(),
                "Confirm enabled for nonexistent remote-tracking ref"
            );
            assert!(i18n::plan_note_text(&plan.blockers[0]).contains("fetch"));
        });
        let bounds = e2e::confirm_bounds(window.window_id()).expect("visible Set Upstream");
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
        paint(cx, window);
        cx.simulate_keystrokes(window, "enter");
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).set_upstream_modal().is_some()));
        assert_eq!(
            std::fs::read(repo.join(".git/config")).unwrap(),
            config_before
        );
    }
    for requested in ["local-upstream", "origin/missing"] {
        if requested.starts_with("origin/") {
            git(repo, &["update-ref", "refs/remotes/origin/missing", "HEAD"]);
        }
        type_name(cx, window, &input, requested);
        assert!(cx.read(|cx| app
            .read(cx)
            .set_upstream_modal()
            .unwrap()
            .plan
            .plan()
            .unwrap()
            .blockers
            .is_empty()));
    }
    assert_eq!(
        std::fs::read(repo.join(".git/config")).unwrap(),
        config_before
    );
    assert_eq!(repo_fingerprint(repo), before);
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    drop(input);
    unmount(cx, app, window);
}
