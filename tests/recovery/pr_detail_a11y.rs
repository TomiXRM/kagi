//! #1255/#1256/#1257/#1259/#1260: native renderer and keyboard regressions.
//! Attribute probes are installed beside real attributes, not AX/VoiceOver readback.
use crate::pr_fields_focus::OfflineGh;
use crate::recovery_layout::contained;
use gpui::{point, px, size, Modifiers, Role, VisualTestAppContext};
use kagi::ui::{
    button_style::ButtonRole,
    e2e::{self, pr_detail},
    i18n::{Lang, Msg},
    pr_mode::PrView,
};
use kagi_ui_core::theme;

#[path = "pr_detail_a11y_support.rs"]
pub(crate) mod support;
use support::*;

const DRAFT: &str = "unsent 日本語 draft";
const HUNK: &str = "@@ -0,0 +1,3 @@\n+one\n+two\n+three";
const COPY: &str = "pr-convo-hunk-copy-7000";
const APPLY: &str = "pr-convo-apply-suggestion-7000";
const MODE: &str = "pr-composer-mode-toggle-button";

pub fn scenario_pr_detail_a11y_1255(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, cell, repo| {
        cell.type_draft(cx);
        let draft = cell.draft(cx);
        let fingerprint = crate::macos::repo_fingerprint(repo);
        for conflict in [false, true] {
            cell.app.update(cx, |a, cx| {
                a.ui_mut().unwrap().pr_mode.as_mut().unwrap().tabs[0]
                    .pr
                    .mergeable = if conflict {
                    kagi_domain::github::Mergeable::Conflicting
                } else {
                    kagi_domain::github::Mergeable::Clean
                };
                a.pr_mode_show(PrView::Overview, cx);
            });
            cell.paint(cx);
            let (role, tabs) = cx.read(|cx| cell.app.read(cx).pr_detail_tabs_for_e2e());
            assert_eq!(role, Some(Role::TabList), "one PR detail TabList");
            assert_eq!(tabs.len(), if conflict { 5 } else { 4 });
            assert_eq!(tabs.iter().filter(|tab| tab.2).count(), 1);
            assert!(tabs
                .iter()
                .all(|tab| tab.0 == Role::Tab && !tab.1.is_empty()));
            let ids = [
                "pr-view-overview",
                "pr-view-review",
                "pr-view-diff",
                "pr-view-commits",
                "pr-view-conflicts",
            ];
            let viewport = gpui::Bounds::new(
                point(px(0.), px(0.)),
                size(px(cell.dimensions.0), px(cell.dimensions.1)),
            );
            for id in &ids[..tabs.len()] {
                let bounds = cell.bounds(id);
                assert!(bounds.size.height >= px(20.));
                contained(viewport, bounds, id);
            }
            let reached = cx
                .update_window(cell.win, |_, window, cx| {
                    let root = cell.app.read(cx).root_focus.clone().unwrap();
                    root.focus(window, cx);
                    (0..160).any(|_| {
                        window.focus_next(cx);
                        cell.app.read(cx).pr_detail_tab_focused_for_e2e(window) == Some(0)
                    })
                })
                .unwrap();
            assert!(reached, "Tab traversal enters the selected PR detail cell");
            for (key, expected) in [
                ("right", 1),
                ("left", 0),
                ("end", tabs.len() - 1),
                ("home", 0),
            ] {
                crate::keyboard_nav::keys(cx, cell.win, key);
                cell.paint(cx);
                let (_, after) = cx.read(|cx| cell.app.read(cx).pr_detail_tabs_for_e2e());
                assert_eq!(
                    after.iter().position(|tab| tab.3),
                    Some(expected),
                    "{key} moves roving focus"
                );
                assert_eq!(
                    after.iter().position(|tab| tab.2),
                    Some(0),
                    "arrows do not activate async views"
                );
                assert_eq!(after.iter().filter(|tab| tab.2).count(), 1);
            }
            for (slot, id) in ids[..tabs.len()].iter().enumerate() {
                cx.update_window(cell.win, |_, window, cx| {
                    cell.app
                        .update(cx, |a, cx| a.focus_pr_detail_tab_for_e2e(slot, window, cx))
                })
                .unwrap();
                for key in ["enter", "space"] {
                    let before = pr_detail::activation_count(id);
                    let bounds = cell.bounds(id);
                    crate::keyboard_nav::keys(cx, cell.win, key);
                    cell.paint(cx);
                    assert_eq!(pr_detail::activation_count(id), before + 1);
                    assert_eq!(cell.bounds(id), bounds);
                    let (_, selected) = cx.read(|cx| cell.app.read(cx).pr_detail_tabs_for_e2e());
                    assert_eq!(selected.iter().position(|tab| tab.2), Some(slot));
                    assert_eq!(selected.iter().filter(|tab| tab.2).count(), 1);
                }
            }
        }
        assert_eq!(
            cell.draft(cx),
            draft,
            "tab navigation retains real Input and draft"
        );
        assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
        crate::keyboard_nav::keys(cx, cell.win, "escape");
        cell.paint(cx);
        assert!(
            cx.read(|cx| cell.app.read(cx).pr_mode().unwrap().active.is_none()),
            "Escape returns to PR Home"
        );
        cx.update_window(cell.win, |_, window, cx| {
            let app = cell.app.read(cx);
            assert_eq!(
                app.pr_detail_tab_focused_for_e2e(window),
                None,
                "hidden detail cells relinquish focus"
            );
            assert!(
                app.root_focus.as_ref().unwrap().is_focused(window),
                "hidden detail tab focus returns to root"
            );
        })
        .unwrap();
        cell.app.update(cx, |a, cx| {
            let pr = a.pr_mode().unwrap().tabs[0].pr.clone();
            a.pr_mode_open(&pr, cx);
        });
        cell.paint(cx);
        assert_eq!(
            cell.draft(cx),
            draft,
            "Home departure/reopen retains the real composer and draft"
        );
    });
}

fn cancel_merge_plan(cx: &mut VisualTestAppContext, cell: &Cell) {
    settle_plan(cx, &cell.app);
    cell.paint(cx);
    assert!(
        cx.update_window(cell.win, |_, window, cx| {
            let app = cell.app.read(cx);
            assert!(
                app.pr_merge_modal().unwrap().plan.destructive,
                "branch-deleting Merge retains its destructive interaction role"
            );
            let modal = app.modal_focus.as_ref().unwrap();
            modal.is_focused(window) || modal.contains_focused(window, cx)
        })
        .unwrap(),
        "accepted deleting Merge plan hands focus to the alert, not the workspace root"
    );
    let opened = pr_detail::activation_count("pr-mode-merge");
    crate::keyboard_nav::keys(cx, cell.win, "escape");
    cell.paint(cx);
    assert!(cx.read(|cx| cell.app.read(cx).pr_merge_modal().is_none()));
    assert_eq!(pr_detail::activation_count("pr-mode-merge"), opened);
}

pub fn scenario_pr_detail_a11y_1256(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, cell, repo| {
        cell.button("pr-mode-title", Msg::PrModeShowDescription);
        cell.button("pr-mode-merge", Msg::PrModeMerge);
        let github = pr_detail::control("pr-mode-open-gh").unwrap();
        assert_eq!(github.role, Some(Role::Link));
        assert_eq!(
            github.name.as_deref(),
            Some(Msg::PrOpenOnGitHub.t().as_ref())
        );
        let before = crate::macos::repo_fingerprint(repo);
        for key in ["enter", "space"] {
            let toggled = pr_detail::control("pr-mode-title").unwrap().toggled;
            cell.activate(cx, "pr-mode-title", "pr-mode-title", key);
            assert_ne!(
                pr_detail::control("pr-mode-title").unwrap().toggled,
                toggled
            );
            cell.activate(cx, "pr-mode-open-gh", "pr-mode-open-gh", key);
            assert_eq!(
                pr_detail::take_opened_url().as_deref(),
                Some("https://github.com/example/repo/pull/7")
            );
            cell.activate(cx, "pr-mode-merge", "pr-mode-merge", key);
            cancel_merge_plan(cx, cell);
            // Opening approval never authorizes transport or local cleanup.
            assert_eq!(crate::macos::repo_fingerprint(repo), before);
        }
        for mergeable in [
            kagi_domain::github::Mergeable::Clean,
            kagi_domain::github::Mergeable::Conflicting,
        ] {
            cell.app.update(cx, |a, cx| {
                a.ui_mut().unwrap().pr_mode.as_mut().unwrap().tabs[0]
                    .pr
                    .mergeable = mergeable;
                cx.notify();
            });
            cell.paint(cx);
            cell.button("pr-mode-merge", Msg::PrModeMerge);
            assert!(!pr_detail::control("pr-mode-merge").unwrap().disabled);
            cell.activate(cx, "pr-mode-merge", "pr-mode-merge", "enter");
            cancel_merge_plan(cx, cell);
            assert_eq!(crate::macos::repo_fingerprint(repo), before);
        }
        for (draft, held) in [(true, false), (false, true)] {
            cell.app.update(cx, |a, cx| {
                a.ui_mut().unwrap().pr_mode.as_mut().unwrap().tabs[0]
                    .pr
                    .is_draft = draft;
                a.set_pr_merge_held_for_e2e(7, held);
                cx.notify();
            });
            e2e::clear_control_bounds(cell.win.window_id(), "pr-mode-merge");
            cell.paint(cx);
            assert!(
                pr_detail::control("pr-mode-merge").is_none(),
                "unavailable merge has no AX action"
            );
            assert!(
                e2e::control_bounds(cell.win.window_id(), "pr-mode-merge").is_none(),
                "unavailable merge has no pointer target"
            );
        }
        cell.app
            .update(cx, |a, _| a.set_pr_merge_held_for_e2e(7, false));
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            0,
            "header approval never executes"
        );
    });
}

pub fn scenario_pr_detail_a11y_1257(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, cell, repo| {
        cell.app
            .update(cx, |a, cx| a.pr_mode_show(PrView::Review, cx));
        cell.paint(cx);
        cell.button(COPY, Msg::PrHunkCopy);
        cell.button(APPLY, Msg::PrApplySuggestion);
        for id in [COPY, APPLY] {
            assert!(pr_detail::control(id).unwrap().tab_stop);
            let bounds = cell.reveal_review(cx, id);
            assert!(bounds.size.width >= px(20.) && bounds.size.height >= px(20.));
        }
        let copy_size = cell.bounds(COPY).size;
        assert!(
            f32::from(copy_size.width - theme::scaled_px(24.)) >= -0.5
                && f32::from(copy_size.height - theme::scaled_px(24.)) >= -0.5,
            "Copy size {copy_size:?} preserves 24 reference px within native pixel rounding"
        );
        let before = crate::macos::repo_fingerprint(repo);
        let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
        let copy_bounds = cell.reveal_review(cx, COPY);
        cx.simulate_mouse_down(
            cell.win,
            copy_bounds.center(),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            cell.win,
            copy_bounds.center() + point(px(-40.), px(35.)),
            Some(gpui::MouseButton::Left),
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            cell.win,
            copy_bounds.center() + point(px(-40.), px(35.)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        assert!(
            !cx.update_window(cell.win, |_, window, cx| pr_detail::has_text_selection(
                window, cx
            ))
            .unwrap(),
            "Copy mouse-down must not start text selection"
        );
        for key in ["enter", "space"] {
            cell.activate(cx, COPY, COPY, key);
            assert_eq!(
                cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap()),
                HUNK
            );
            cell.activate(cx, APPLY, APPLY, key);
            assert!(
                cx.read(|cx| cell.app.read(cx).apply_suggestion_modal().is_some()),
                "Apply opens existing plan"
            );
            assert_eq!(crate::macos::repo_fingerprint(repo), before);
            // Use the focus handed off by the opener, not a test-side refocus.
            let opened = pr_detail::activation_count(APPLY);
            crate::keyboard_nav::keys(cx, cell.win, "enter");
            cell.paint(cx);
            assert!(cx.read(|cx| cell.app.read(cx).apply_suggestion_modal().is_some()));
            assert_eq!(
                pr_detail::activation_count(APPLY),
                opened,
                "Return must not reactivate the opener through key-up"
            );
            assert_eq!(crate::macos::repo_fingerprint(repo), before);
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
                receipts
            );
            crate::keyboard_nav::keys(cx, cell.win, "escape");
            cell.paint(cx);
            assert!(cx.read(|cx| cell.app.read(cx).apply_suggestion_modal().is_none()));
        }
    });
}

fn mode_toggle(cx: &mut VisualTestAppContext, cell: &Cell, id: &str, geometry: &str) {
    cell.button(id, Msg::ComposerPreview);
    for key in ["enter", "space"] {
        cell.activate(cx, id, geometry, key);
        cell.button(id, Msg::ComposerPreview);
        assert_eq!(pr_detail::control(id).unwrap().toggled, Some(true));
        cell.activate(cx, id, geometry, key);
        cell.button(id, Msg::ComposerPreview);
        assert_eq!(pr_detail::control(id).unwrap().toggled, Some(false));
    }
}

pub fn scenario_pr_detail_a11y_1259(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, cell, _| {
        cell.type_draft(cx);
        let draft = cell.draft(cx);
        mode_toggle(cx, cell, MODE, "pr-composer-mode-toggle");
        assert_eq!(cell.draft(cx), draft);
        cx.update_window(cell.win, |_, window, cx| {
            let input = cell.app.read(cx).pr_comment_input.clone().unwrap();
            input.update(cx, |input, cx| input.focus(window, cx));
        })
        .unwrap();
        crate::keyboard_nav::keys(cx, cell.win, "secondary-z");
        assert_eq!(cell.draft(cx).1, "", "preview preserves PR undo history");
        crate::keyboard_nav::keys(cx, cell.win, "secondary-shift-z");
        assert_eq!(cell.draft(cx), draft, "PR redo returns the same real Input");
        cell.app.update(cx, |a, cx| {
            a.seed_issue_composer_for_e2e(cx);
            a.show_issues_mode(cx);
        });
        cell.paint(cx);
        // Enter is the production reveal path; undoing to empty must not hide the body.
        cx.update_window(cell.win, |_, window, cx| {
            cell.app
                .update(cx, |a, cx| a.focus_issue_title_for_e2e(window, cx))
        })
        .unwrap();
        crate::keyboard_nav::keys(cx, cell.win, "enter");
        cell.paint(cx);
        cx.update_window(cell.win, |_, window, cx| {
            cell.app
                .update(cx, |a, cx| a.insert_issue_body_for_e2e(DRAFT, window, cx))
        })
        .unwrap();
        cell.paint(cx);
        let issue_draft = cx.read(|cx| cell.app.read(cx).issue_composer_snapshot_for_e2e());
        mode_toggle(
            cx,
            cell,
            "issue-composer-mode-toggle",
            "issue-composer-mode-toggle",
        );
        assert_eq!(
            cx.read(|cx| cell.app.read(cx).issue_composer_snapshot_for_e2e().0.body),
            issue_draft.0.body
        );
        for key in ["enter", "space"] {
            cell.button("issue-composer-focus", Msg::IssueFocusEditor);
            cell.activate(cx, "issue-composer-focus", "issue-composer-focus", key);
            cell.button("issue-composer-focus", Msg::IssueExitFocus);
            cell.activate(cx, "issue-composer-focus", "issue-composer-focus", key);
        }
        issue_undo(cx, cell, None);
        cell.app.update(cx, |a, cx| {
            a.seed_issue_navigation_for_e2e(cx);
            a.seed_issue_reply_for_e2e(4, cx);
        });
        cell.paint(cx);
        cx.update_window(cell.win, |_, window, cx| {
            cell.app.update(cx, |a, cx| {
                a.replace_issue_reply_body_for_e2e(4, DRAFT, window, cx)
            })
        })
        .unwrap();
        cell.paint(cx);
        mode_toggle(
            cx,
            cell,
            "issue-reply-composer-mode-toggle",
            "issue-reply-mode-toggle",
        );
        cell.button("issue-reply-composer-focus", Msg::IssueFocusEditor);
        cell.activate(
            cx,
            "issue-reply-composer-focus",
            "issue-reply-composer-focus",
            "enter",
        );
        cell.button("issue-reply-composer-focus", Msg::IssueExitFocus);
        cell.activate(
            cx,
            "issue-reply-composer-focus",
            "issue-reply-composer-focus",
            "space",
        );
        assert_eq!(
            cx.read(|cx| cell.app.read(cx).issue_reply_snapshot_for_e2e(4).body),
            DRAFT
        );
        issue_undo(cx, cell, Some(4));
    });
}

pub fn scenario_pr_detail_a11y_1260_en(cx: &mut VisualTestAppContext) {
    submit_matrix(cx, Lang::En);
}

pub fn scenario_pr_detail_a11y_1260_ja(cx: &mut VisualTestAppContext) {
    submit_matrix(cx, Lang::Ja);
}

fn submit_matrix(cx: &mut VisualTestAppContext, language: Lang) {
    matrix_langs(cx, &[language], |cx, cell, repo| {
        let disabled = pr_detail::control("pr-comment-post").unwrap();
        assert!(disabled.disabled);
        let count = pr_detail::activation_count("pr-comment-post");
        cx.simulate_click(
            cell.win,
            cell.bounds("pr-comment-post").center(),
            Modifiers::none(),
        );
        cell.paint(cx);
        assert_eq!(pr_detail::activation_count("pr-comment-post"), count);
        cell.type_draft(cx);
        let before = cell.draft(cx);
        let fingerprint = crate::macos::repo_fingerprint(repo);
        assert_eq!(
            pr_detail::control("pr-comment-post").unwrap().semantic,
            Some(ButtonRole::Primary)
        );
        assert!(!pr_detail::control("pr-comment-post").unwrap().disabled);
        assert!(
            f32::from(cell.bounds("pr-comment-post").size.height - theme::scaled_px(36.)).abs()
                <= 0.5,
            "submit height preserves 36 reference px within native pixel rounding"
        );
        cell.button("pr-comment-post", Msg::PrCommentPost);
        cell.button("pr-review-request-changes", Msg::PrReviewRequestChanges);
        assert_eq!(
            pr_detail::control("pr-review-request-changes")
                .unwrap()
                .semantic,
            Some(ButtonRole::Warning)
        );
        let mut accents = Vec::new();
        for accent in [theme::SystemAccent::Blue, theme::SystemAccent::Purple] {
            let _accent = theme::AccentInjection::new(accent, Some(true));
            cx.update(|cx| theme::sync_gpui_component_theme(cx));
            cell.paint(cx);
            let outside = point(px(1.), px(1.));
            cx.simulate_mouse_move(cell.win, outside, None, Modifiers::none());
            cell.paint(cx);
            let rest = fill(cx, cell, "pr-comment-post");
            let expected = accent.rgb(theme::theme().dark);
            for (got, wanted) in rest[..3].iter().zip([
                (expected >> 16) as u8,
                (expected >> 8) as u8,
                expected as u8,
            ]) {
                assert!(
                    got.abs_diff(wanted) <= 2,
                    "native submit {rest:?}, expected #{expected:06x}"
                );
            }
            let bounds = cell.bounds("pr-comment-post");
            cx.simulate_mouse_move(cell.win, bounds.center(), None, Modifiers::none());
            cell.paint(cx);
            let hover = fill(cx, cell, "pr-comment-post");
            cx.simulate_mouse_down(
                cell.win,
                bounds.center(),
                gpui::MouseButton::Left,
                Modifiers::none(),
            );
            cell.paint(cx);
            let pressed = fill(cx, cell, "pr-comment-post");
            cx.simulate_mouse_move(
                cell.win,
                outside,
                Some(gpui::MouseButton::Left),
                Modifiers::none(),
            );
            cx.simulate_mouse_up(
                cell.win,
                outside,
                gpui::MouseButton::Left,
                Modifiers::none(),
            );
            assert_ne!(rest, hover);
            assert_ne!(rest, pressed);
            assert_ne!(hover, pressed);
            accents.push(rest);
        }
        assert_ne!(
            accents[0], accents[1],
            "normal submit follows system accent"
        );
        assert_eq!(
            cell.draft(cx),
            before,
            "paint states do not post or alter draft"
        );
        assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
        // Reuse the established fake-gh stdin/URL fixture contract; never post online.
        let _writes = OfflineGh::with_script(
            r#"#!/bin/sh
case "$1 $2" in
  'api user') echo octocat ;;
  'label list') echo '[]' ;;
  'pr comment'|'issue comment') cat > /dev/null; echo https://example.invalid/example/fixture/issues/4 ;;
  'issue create') cat > /dev/null; echo https://example.invalid/example/fixture/issues/9 ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#,
        );
        for key in ["enter", "space"] {
            cell.type_draft(cx);
            cell.activate(cx, "pr-comment-post", "pr-comment-post", key);
            settle_write(cx, &cell.app);
            cell.paint(cx);
            assert_eq!(
                cell.draft(cx).1,
                "",
                "successful offline post consumes only its submitted draft"
            );
            assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
        }
        cell.app.update(cx, |a, cx| {
            a.seed_issue_composer_for_e2e(cx);
            a.show_issues_mode(cx);
        });
        cell.paint(cx);
        cx.update_window(cell.win, |_, window, cx| {
            cell.app
                .update(cx, |a, cx| a.focus_issue_title_for_e2e(window, cx))
        })
        .unwrap();
        crate::keyboard_nav::keys(cx, cell.win, "enter");
        cell.paint(cx);
        disabled_submit(cx, cell, "issue-composer-submit", Msg::IssueCreate);
        cx.update_window(cell.win, |_, window, cx| {
            cell.app
                .update(cx, |a, cx| a.insert_issue_body_for_e2e(DRAFT, window, cx))
        })
        .unwrap();
        cell.paint(cx);
        enabled_submit(cx, cell, "issue-composer-submit", Msg::IssueCreate);
        cell.app.update(cx, |a, cx| {
            a.seed_issue_navigation_for_e2e(cx);
            a.seed_issue_reply_for_e2e(4, cx);
        });
        cell.paint(cx);
        disabled_submit(cx, cell, "issue-reply-composer-submit", Msg::IssueReply);
        cx.update_window(cell.win, |_, window, cx| {
            cell.app.update(cx, |a, cx| {
                a.replace_issue_reply_body_for_e2e(4, DRAFT, window, cx)
            })
        })
        .unwrap();
        cell.paint(cx);
        enabled_submit(cx, cell, "issue-reply-composer-submit", Msg::IssueReply);
        assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
    });
}
