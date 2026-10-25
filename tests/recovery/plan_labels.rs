//! #1343–#1345: directional actions, bounded footers and lossless subjects.
use crate::macos::{build_fixture, git, repo_fingerprint, unmount};
use crate::presentation_support::{each_presentation, mount, paint, Presentation};
use gpui::{px, AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{button_style, dialog_a11y, e2e, modals::RebaseCurrentOntoModal, KagiApp};
use kagi_domain::plan_note::{PlanTitle, RebaseTitle};
use kagi_ui_core::{i18n, theme};
use std::{path::Path, sync::Arc, time::Duration};
#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn modal(
    repo: &Path,
    branch: &str,
    onto: &str,
    replay: bool,
    armed: bool,
    blocked: bool,
) -> RebaseCurrentOntoModal {
    let op = kagi_git::Operation::RebaseCurrentOnto { onto: onto.into() };
    let mut plan = kagi_git::Backend::open(repo).unwrap().plan(&op).unwrap();
    if let Some(scope) = &plan.rebase_scope {
        assert_eq!(
            plan.warnings.iter().any(|note| matches!(
                note,
                kagi_domain::plan_note::PlanNote::Rebase(
                    kagi_domain::plan_note::RebaseNote::MayConflict
                )
            )),
            scope.total_commits > 0,
            "only a nonempty rebase sequence warrants a conflict caution"
        );
    }
    plan.title = PlanTitle::Rebase(if replay {
        RebaseTitle::ReplayOnto {
            branch: branch.into(),
            onto: onto.into(),
        }
    } else {
        RebaseTitle::RebaseCurrentOnto {
            branch: branch.into(),
            onto: onto.into(),
        }
    });
    plan.destructive = replay;
    if !blocked {
        plan.blockers.clear();
    }
    RebaseCurrentOntoModal {
        op: if replay {
            kagi_git::Operation::ReplayOnto {
                branch: branch.into(),
                onto: onto.into(),
            }
        } else {
            op
        },
        onto: onto.into(),
        branch: branch.into(),
        plan: Arc::new(plan),
        error: None,
        confirm_armed: armed,
    }
}
fn focus_confirm(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    crate::recovery_operations::press_key(cx, app, window, "f19");
    for _ in 0..80 {
        button_style::modal_focus_probe();
        cx.simulate_keystrokes(window, "tab");
        cx.run_until_parked();
        paint(cx, window);
        cx.simulate_keystrokes(window, "f19");
        if button_style::modal_focus_probe() == Some("plan-confirm") {
            return;
        }
    }
    panic!("Tab must reach the real confirmation Button");
}
fn rebase_keys(cx: &mut VisualTestAppContext, branch: &str, onto: &str, key: &str) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-qb", "feature", "HEAD~1"]);
    std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "feature work"]);
    git(repo, &["checkout", "-q", branch]);
    let before = repo_fingerprint(repo);
    let target = git_fixture::git_output(repo, &["rev-parse", onto]);
    let (app, window) = mount(cx, repo, (940., 660.));
    app.update(cx, |app, cx| app.open_rebase_modal(onto.into(), cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).planning.is_none()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rebase planning did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    paint(cx, window);
    cx.read(|cx| {
        let modal = app.read(cx).rebase_current_onto_modal().unwrap();
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert!(matches!(&modal.op, kagi_git::Operation::RebaseCurrentOnto { onto: actual } if actual == onto));
        assert!(modal.plan.rebase_scope.as_ref().unwrap().total_commits > 0);
        assert!(modal.plan.warnings.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Rebase(
                kagi_domain::plan_note::RebaseNote::MayConflict
            )
        )), "a real rebase sequence must retain its conflict caution");
    });
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "planning/rendering is read-only"
    );
    focus_confirm(cx, &app, window);
    crate::keyboard_nav::keys(cx, window, key);
    crate::recovery_operations::wait_idle(cx, &app);
    assert!(cx.read(|cx| app.read(cx).rebase_current_onto_modal().is_none()));
    git(repo, &["merge-base", "--is-ancestor", &target, "HEAD"]);
    let entries: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "rebase")
        .collect();
    assert_eq!(entries.len(), 1, "one key dispatch, one execution receipt");
    assert!(matches!(
        entries[0].outcome,
        kagi_git::oplog::OpOutcome::Success { .. }
    ));
    unmount(cx, app, window);
}

pub fn scenario_rebase_plan_labels(cx: &mut VisualTestAppContext) {
    let _presentation = Presentation::keep();
    let fixture = build_fixture();
    let fingerprint = repo_fingerprint(fixture.path());
    each_presentation(cx, fixture.path(), |cx, app, window| {
        for (branch, onto) in [("feature", "main"), ("main", "feature")] {
            app.update(cx, |app, cx| {
                app.set_rebase_current_onto_modal(modal(
                    fixture.path(),
                    branch,
                    onto,
                    false,
                    false,
                    false,
                ));
                cx.notify();
            });
            paint(cx, window);
            let action = button_style::recorded_modal_button("plan-confirm").unwrap();
            assert_eq!(action.role, Role::Button);
            assert_eq!(
                action.label,
                match i18n::lang() {
                    i18n::Lang::En => format!("Rebase {branch}"),
                    i18n::Lang::Ja => format!("{branch} を rebase"),
                }
            );
            assert_eq!(
                dialog_a11y::recorded_dialog("plan-card").unwrap().actions[0].1,
                action.label
            );
            assert_eq!(
                repo_fingerprint(fixture.path()),
                fingerprint,
                "render must not write refs"
            );
        }
    });
    theme::set_zoom(1.);
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        for (branch, onto) in [("feature", "main"), ("main", "feature")] {
            for key in ["enter", "space"] {
                rebase_keys(cx, branch, onto, key);
            }
        }
    }
}

pub fn scenario_plan_footer_long_branch(cx: &mut VisualTestAppContext) {
    let _presentation = Presentation::keep();
    let fixture = build_fixture();
    let fingerprint = repo_fingerprint(fixture.path());
    each_presentation(cx, fixture.path(), |cx, app, window| {
        for length in [1, 80, 200] {
            let branch = "b".repeat(length);
            for (replay, armed) in [(false, false), (true, false), (true, true)] {
                for blocked in [false, true] {
                    e2e::clear_control_bounds(window.window_id(), "plan-confirm-caption");
                    app.update(cx, |app, cx| {
                        app.set_rebase_current_onto_modal(modal(
                            fixture.path(),
                            &branch,
                            "main",
                            replay,
                            armed,
                            blocked,
                        ));
                        cx.notify();
                    });
                    paint(cx, window);
                    let footer = e2e::control_bounds(window.window_id(), "modal-footer").unwrap();
                    let cancel = e2e::control_bounds(window.window_id(), "plan-cancel").unwrap();
                    let confirm = e2e::control_bounds(window.window_id(), "plan-confirm").unwrap();
                    for bounds in [cancel, confirm] {
                        assert!(bounds.left() >= footer.left() - px(1.) && bounds.right() <= footer.right() + px(1.), "{length}/{replay}/{armed}/{blocked}: action outside footer: {bounds:?} footer {footer:?}");
                    }
                    assert!(cancel.right() <= confirm.left(), "actions overlap");
                    let action = button_style::recorded_modal_button("plan-confirm").unwrap();
                    assert_eq!(action.role, Role::Button);
                    assert_eq!(action.disabled, blocked);
                    if armed {
                        assert_eq!(action.label, i18n::Msg::PlanReplayOntoArmed.t());
                        assert!(
                            e2e::control_bounds(window.window_id(), "plan-confirm-caption")
                                .is_none(),
                            "the complete armed warning must use the unellipsized button"
                        );
                    } else {
                        let caption =
                            e2e::control_bounds(window.window_id(), "plan-confirm-caption")
                                .unwrap();
                        assert!(caption.size.width <= theme::scaled_px(160.) + px(1.));
                        let detail = action.description.unwrap();
                        assert!(
                            detail.contains(&branch) && detail.contains("main"),
                            "full source/destination AX identity"
                        );
                    }
                    if replay {
                        crate::destructive_modal_return::root_return_keeps_modal(
                            cx,
                            app,
                            window,
                            fixture.path(),
                        );
                        assert_eq!(
                            cx.read(|cx| app
                                .read(cx)
                                .rebase_current_onto_modal()
                                .unwrap()
                                .confirm_armed),
                            armed
                        );
                        crate::recovery_operations::press_key(cx, app, window, "escape");
                        assert!(cx.read(|cx| app.read(cx).rebase_current_onto_modal().is_none()));
                    }
                    assert_eq!(repo_fingerprint(fixture.path()), fingerprint);
                }
            }
        }
    });
}

fn subjects() -> Vec<String> {
    let mut subjects: Vec<_> = [71, 72, 73, 300]
        .into_iter()
        .map(|length| format!("{}😀", "日".repeat(length - 1)))
        .collect();
    for suffix in ["first-tail", "second-tail"] {
        subjects.push(format!("{} {suffix}", "日".repeat(72)));
    }
    subjects
}
fn push_subject_case(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    expected: &[String],
    sidebar: bool,
) {
    let before = repo_fingerprint(repo);
    // This scenario tests retained plan-card subject rendering/copy, not the
    // toolbar's ordinary safe Push intent (which intentionally has no card).
    let backend = kagi_git::Backend::open(repo).unwrap();
    let plan = if sidebar {
        backend.plan_push_branch("main", true).unwrap()
    } else {
        backend.plan_push().unwrap()
    };
    app.update(cx, |app, _| {
        app.set_push_modal(kagi::ui::modals::PushPlanModal {
            plan: std::sync::Arc::new(plan),
            error: None,
        });
    });
    e2e::plan_subjects::clear();
    paint(cx, window);
    let plan = cx.read(|cx| app.read(cx).push_modal().unwrap().plan.clone());
    let no_force = i18n::plan_note_text(&kagi_domain::plan_note::PlanNote::Push(
        kagi_domain::plan_note::PushNote::NoForceUsed {
            punct: kagi_domain::plan_note::push::PushPunct::Semicolon,
        },
    ));
    if sidebar {
        assert!(
            plan.warnings.is_empty(),
            "sidebar Push is quiet: {:?}",
            plan.warnings
        );
        let (_, after) = dialog_a11y::recorded_note("plan-state-after").unwrap();
        assert!(
            after.contains(&no_force),
            "AFTER must preserve the no-force guarantee: {after}"
        );
    }
    for subject in expected {
        assert!(
            plan.preview_commits
                .iter()
                .any(|entry| entry.ends_with(subject)),
            "full subject {subject}"
        );
    }
    let rows = e2e::plan_subjects::rows();
    assert_eq!(rows.len(), plan.preview_commits.len());
    for (row, entry) in rows.iter().zip(&plan.preview_commits) {
        assert_eq!(row.role, Role::Label);
        assert_eq!(row.label.as_ref(), entry);
        let bounds =
            e2e::control_bounds(window.window_id(), &format!("modal-commit-{entry}")).unwrap();
        assert!(
            (bounds.size.height - theme::scaled_px(18.)).abs() <= px(1.),
            "fixed row pitch"
        );
    }
    for control in ["plan-card-copy", "plan-commits-copy"] {
        let copy = e2e::control_bounds(window.window_id(), control).unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        for subject in expected {
            assert!(copied.contains(subject), "{control}: full subject");
        }
        if control == "plan-card-copy" && sidebar {
            assert!(
                copied.contains(&no_force),
                "Copy all must preserve the no-force guarantee"
            );
            assert!(
                !copied.contains("\nwarning:"),
                "sidebar Push must not copy a caution"
            );
        }
        if control == "plan-commits-copy" {
            assert_eq!(copied, plan.preview_commits.join("\n"));
        }
    }
    let row = e2e::control_bounds(
        window.window_id(),
        &format!("modal-commit-{}", plan.preview_commits[0]),
    )
    .unwrap();
    e2e::text_caps::clear();
    cx.simulate_mouse_move(window, row.center(), None, gpui::Modifiers::none());
    for _ in 0..3 {
        cx.advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        paint(cx, window);
    }
    assert_eq!(
        e2e::text_caps::tooltip().as_deref(),
        Some(plan.preview_commits[0].as_str()),
        "actual hover reveal"
    );
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "render/copy has no local writes"
    );
}

pub fn scenario_push_plan_subjects(cx: &mut VisualTestAppContext) {
    let _presentation = Presentation::keep();
    for upstream in [false, true] {
        let fixture = build_fixture();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(
            fixture.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        if upstream {
            git(fixture.path(), &["push", "-qu", "origin", "main"]);
        }
        let remote_before = git_fixture::git_output(
            remote.path(),
            &["for-each-ref", "--format=%(refname):%(objectname)"],
        );
        let expected = subjects();
        for subject in &expected {
            git(fixture.path(), &["commit", "--allow-empty", "-qm", subject]);
        }
        each_presentation(cx, fixture.path(), |cx, app, window| {
            for sidebar in [false, true] {
                push_subject_case(cx, app, window, fixture.path(), &expected, sidebar);
            }
        });
        assert_eq!(
            git_fixture::git_output(
                remote.path(),
                &["for-each-ref", "--format=%(refname):%(objectname)"]
            ),
            remote_before,
            "no transport write without approval"
        );
    }
}
