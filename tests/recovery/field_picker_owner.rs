//! #904 review: the field picker belongs to the tab it was opened from.
//! Leaving that tab drops the picker (it is repo-scoped, ADR-0197), so an
//! Apply after a tab switch has nothing to apply: tab B's composer takes
//! none of A's picks, A's composer keeps what it had, and no PR edit is sent
//! from B — even when B shows a PR with the same number. The picker also
//! carries its owner, so Apply reaches only the opening tab should it ever
//! outlive the switch.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::modals::PrField;
use kagi::ui::{e2e, KagiApp};
use kagi_git::oplog::read_oplog_tail_for_repo;

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::recovery_operations::wait_idle;

/// Labels and assignees are offered; a PR edit that reaches `gh` fails, so
/// an edit that should not have been sent leaves a receipt behind.
fn install_gh(dir: &Path) -> OfflineGh {
    OfflineGh::with_script(&format!(
        r#"#!/bin/sh
case "$1 $2" in
  'label list') cat '{labels}' ;;
  'api '*) echo octocat; echo hubot ;;
  'pr edit') echo 'refused: sent from the wrong tab' >&2; exit 1 ;;
  'pr '*) echo '{{}}' ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#,
        labels = dir.join("labels.json").display(),
    ))
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw window");
}

fn wait_candidates(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !cx.read(|cx| {
        app.read(cx)
            .pr_fields_modal()
            .is_some_and(|modal| modal.candidates.is_some())
    }) {
        assert!(
            std::time::Instant::now() < deadline,
            "the candidate read never landed"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Toggle `value` in the open picker, switch to tab `to`, run `then` there,
/// and press Apply.
fn apply_from_another_tab(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    value: &str,
    to: usize,
    then: impl FnOnce(&mut VisualTestAppContext),
) {
    wait_candidates(cx, app);
    app.update(cx, |app, cx| app.pr_fields_toggle(value.into(), cx));
    app.update(cx, |app, cx| app.switch_repo(to, cx));
    paint(cx, window);
    then(cx);
    assert!(
        cx.read(|cx| app.read(cx).pr_fields_modal().is_none()),
        "leaving the tab drops its picker"
    );
    app.update(cx, |app, cx| app.confirm_pr_fields(cx));
    cx.run_until_parked();
}

fn open_pr(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    pr: &kagi_domain::github::PullRequest,
) {
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    app.update(cx, |app, cx| app.pr_mode_open(pr, cx));
    wait_idle(cx, app);
    paint(cx, window);
}

fn new_issue_leg(cx: &mut VisualTestAppContext) {
    let a = build_fixture();
    let b = build_fixture();
    let gh_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        gh_dir.path().join("labels.json"),
        r#"[{"name":"bug","color":"d73a4a"},{"name":"docs","color":"0075ca"}]"#,
    )
    .unwrap();
    let gh = install_gh(gh_dir.path());
    let (app, window) = mount(cx, &a.path().canonicalize().unwrap());
    app.update(cx, |app, cx| {
        assert!(app.open_repository(b.path().to_path_buf(), cx));
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
        app.switch_repo(0, cx);
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    paint(cx, window);

    app.update(cx, |app, cx| {
        app.open_issue_fields_modal(PrField::Labels, cx)
    });
    apply_from_another_tab(cx, window, &app, "docs", 1, |_| {});
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_create_fields_for_e2e()),
        Default::default(),
        "tab B's composer must not take A's picks"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_create_fields_for_e2e()),
        Default::default(),
        "a dropped picker applies nothing to the tab that opened it either"
    );

    drop(gh);
    unmount(cx, app, window);
}

fn pr_leg(cx: &mut VisualTestAppContext) {
    let a = build_fixture();
    let b = build_fixture();
    let a_path = a.path().canonicalize().unwrap();
    let b_path = b.path().canonicalize().unwrap();
    let gh_dir = tempfile::tempdir().unwrap();
    std::fs::write(gh_dir.path().join("labels.json"), "[]").unwrap();
    let gh = install_gh(gh_dir.path());
    let (app, window) = mount(cx, &a_path);
    // The same PR number in both tabs: only the owner tells them apart.
    let pr = pull_request(7, "field picker", "main");
    app.update(cx, |app, cx| {
        assert!(app.open_repository(b.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    open_pr(cx, window, &app, &pr);
    app.update(cx, |app, cx| {
        app.open_pr_fields_modal(PrField::Reviewers, cx)
    });
    // Tab B shows the same PR number when Apply runs.
    apply_from_another_tab(cx, window, &app, "octocat", 1, |cx| {
        open_pr(cx, window, &app, &pr)
    });
    wait_idle(cx, &app);
    for repo in [&a_path, &b_path] {
        assert!(
            read_oplog_tail_for_repo(repo, 100)
                .iter()
                .all(|entry| entry.op != "pr-edit"),
            "no PR edit may be sent from a tab the picker was not opened in"
        );
    }

    drop(gh);
    unmount(cx, app, window);
}

pub fn scenario_field_picker_owner(cx: &mut VisualTestAppContext) {
    new_issue_leg(cx);
    pr_leg(cx);
    eprintln!(
        "[gui-e2e] PASS field_picker_owner: a tab switch drops the picker; neither tab's composer changes and no PR edit is sent"
    );
}
