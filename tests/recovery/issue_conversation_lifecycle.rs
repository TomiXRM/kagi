//! #1091 lifecycle consumer proofs through real producer and real app loader.
use crate::evidence_support::deferred;
use crate::issue_conversation_selection::support::*;
use crate::macos::{build_fixture, repo_fingerprint};
use gpui::{Focusable, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::PrListSnapshot;

const BASE_B: &str = "github.com/conversation/owner-b";

fn request(cx: &mut VisualTestAppContext, app: &NativeApp, win: gpui::AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.load_github_issue_detail(4, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}
fn retarget(
    cx: &mut VisualTestAppContext,
    app: &NativeApp,
    win: gpui::AnyWindowHandle,
    base: &str,
) -> String {
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(
        kagi_domain::github::IssueListSnapshot {
            issues: Vec::new(),
            mentioned_numbers: Vec::new(),
            base_repo: base.to_string(),
            next_cursor: None,
        },
    )));
    copy_before_paint(cx, win, |_, cx| {
        app.update(cx, |app, cx| e2e::retarget_github_issues(app, base, cx));
    })
}

fn return_to_issues(cx: &mut VisualTestAppContext, app: &NativeApp) {
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(
        kagi_domain::github::IssueListSnapshot {
            issues: Vec::new(),
            mentioned_numbers: Vec::new(),
            base_repo: BASE.to_string(),
            next_cursor: None,
        },
    )));
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    cx.run_until_parked();
}

pub fn scenario_issue_conversation_return_pending(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish("a", "**DEPARTED_DETAIL**", &[]);
    let (fixture, app, win, before) = fixture(cx, BASE);
    let departed = kagi_git::github::issue_detail(fixture.path(), Some(BASE), 4);
    let (task, release_departed) = deferred(cx);
    let _departed_hold = e2e::queue_github_issue_detail(task);
    request(cx, &app, win);
    assert!(visible(cx, win, "issue-mode-detail-loading").is_some());
    assert!(measure(cx, win, BODY).is_none());
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    cx.run_until_parked();

    producer.publish("a", "**RETURNED_DETAIL**", &[]);
    let returned = kagi_git::github::issue_detail(fixture.path(), Some(BASE), 4);
    let (task, release_returned) = deferred(cx);
    let _returned_hold = e2e::queue_github_issue_detail(task);
    return_to_issues(cx, &app);
    assert!(
        visible(cx, win, "issue-mode-detail-loading").is_some() || visible(cx, win, BODY).is_some(),
        "return before acceptance must paint Loading or accepted body, never only the composer"
    );
    assert!(visible(cx, win, "issue-reply-composer").is_some());
    release_departed.send(departed);
    cx.run_until_parked();
    assert!(
        visible(cx, win, "issue-mode-detail-loading").is_some(),
        "the departed generation must not settle the resumed read"
    );
    assert!(measure(cx, win, BODY).is_none());
    release_returned.send(returned);
    cx.run_until_parked();
    assert_eq!(select(cx, win, BODY), "RETURNED_DETAIL");
    assert!(measure(cx, win, "issue-mode-detail-loading").is_none());
    assert_eq!(producer.requests(), vec!["a", "a"]);
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_return_failed(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.fail("a");
    let (fixture, app, win, before) = fixture(cx, BASE);
    request(cx, &app, win);
    assert!(visible(cx, win, "issue-mode-detail-error").is_some());
    assert!(!cx.read(|cx| app.read(cx).ui().github_issue_details.contains_key(&4)));
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    cx.run_until_parked();

    let failed = kagi_git::github::issue_detail(fixture.path(), Some(BASE), 4);
    assert!(
        failed.is_err(),
        "the offline producer delivers a real gh failure"
    );
    let (task, release_failed) = deferred(cx);
    let _failed_hold = e2e::queue_github_issue_detail(task);
    return_to_issues(cx, &app);
    assert!(
        visible(cx, win, "issue-mode-detail-loading").is_some(),
        "return after an unaccepted failure must paint the re-read's Loading chrome"
    );
    release_failed.send(failed);
    cx.run_until_parked();
    assert!(
        visible(cx, win, "issue-mode-detail-error").is_some(),
        "a failed resumed detail must paint error chrome, never only the composer"
    );
    assert!(visible(cx, win, "issue-reply-composer").is_some());
    assert!(measure(cx, win, BODY).is_none());
    assert_eq!(producer.requests(), vec!["a", "a"]);
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_delayed_rejected(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish("a", "**ACCEPTED_A**", &[]);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "ACCEPTED_A");
    let y = measure(cx, win, BODY).unwrap().top();
    producer.publish("a", "**SUPERSEDED**", &[]);
    // The delayed payload was produced and parsed by the strict real gh path.
    // Only its delivery future is held; owner/generation are loader-owned.
    let result = kagi_git::github::issue_detail(fixture.path(), Some(BASE), 4);
    let (task, release) = deferred(cx);
    let _hold = e2e::queue_github_issue_detail(task);
    request(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        "ACCEPTED_A",
        "beginning a held refresh preserves accepted logical selection"
    );
    assert_eq!(
        measure(cx, win, BODY).unwrap().top(),
        y,
        "refresh loading chrome preserves the accepted anchor"
    );
    producer.publish("a", "**CURRENT_A**", &[]);
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "CURRENT_A");
    release.send(result);
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        "CURRENT_A",
        "releasing a superseded generation cannot restore prior producer text/selection"
    );
    assert_eq!(
        select(cx, win, BODY),
        "CURRENT_A",
        "current rendered body remains exact after rejected completion"
    );

    producer.publish("a", "**LATE_OLD_OWNER**", &[]);
    let result = kagi_git::github::issue_detail(fixture.path(), Some(BASE), 4);
    let (task, release) = deferred(cx);
    let _hold_retarget = e2e::queue_github_issue_detail(task);
    request(cx, &app, win);
    producer.publish("b", "**CURRENT_B**", &[]);
    assert_eq!(
        retarget(cx, &app, win, BASE_B),
        POISON,
        "base repository retarget→Copy in one update retires old Copy before paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after base repository retarget cannot copy the retired owner"
    );
    cx.run_until_parked();
    load(cx, &app, win);
    assert_eq!(
        select(cx, win, BODY),
        "CURRENT_B",
        "same issue number is addressed to the new real base repository"
    );
    release.send(result);
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        "CURRENT_B",
        "late old-base completion cannot overwrite current B"
    );
    assert_eq!(select(cx, win, BODY), "CURRENT_B");
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_two_owners(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish("a", "**SESSION_A_ISSUE_4**", &[]);
    producer.publish("b", "**SESSION_B_ISSUE_4**", &[]);
    let (fixture_a, app, win, before_a) = fixture(cx, BASE);
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "SESSION_A_ISSUE_4");
    let owner_a = cx.read(|cx| app.read(cx).active_session().unwrap());
    let fixture_b = build_fixture();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let before_b = repo_fingerprint(&repo_b);
    producer.publish("a", "**A_BACKGROUND_REFRESH**", &[]);
    let result = kagi_git::github::issue_detail(fixture_a.path(), Some(BASE), 4);
    let (task, release) = deferred(cx);
    let _hold = e2e::queue_github_issue_detail(task);
    request(cx, &app, win);
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(PrListSnapshot {
        prs: Vec::new(),
        base_repo: "github.com/example/repo".into(),
        next_cursor: None,
    })));
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| {
                assert!(
                    app.open_repository(repo_b.clone(), cx),
                    "real session B attaches"
                )
            });
        }),
        POISON,
        "session departure→Copy in one update synchronously retires A before paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after session departure cannot copy A"
    );
    cx.run_until_parked();
    configure(cx, &app, BASE_B);
    let owner_b = cx.read(|cx| app.read(cx).active_session().unwrap());
    assert_ne!(
        owner_a, owner_b,
        "two actual attached app sessions, not test-owner aliases"
    );
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "SESSION_B_ISSUE_4");
    release.send(result);
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        "SESSION_B_ISSUE_4",
        "A's background accepted refresh cannot activate its group on B"
    );
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| app.switch_repo(0, cx));
        }),
        POISON,
        "tab switch→Copy in one update cannot leave B live pending paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after tab switch cannot copy retired B"
    );
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    assert_eq!(
        select(cx, win, BODY),
        "A_BACKGROUND_REFRESH",
        "returning A renders its own accepted background payload"
    );
    producer.publish("a", "**DROPPED_A_COMPLETION**", &[]);
    let result = kagi_git::github::issue_detail(fixture_a.path(), Some(BASE), 4);
    let (task, release) = deferred(cx);
    let _drop_hold = e2e::queue_github_issue_detail(task);
    request(cx, &app, win);
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| app.close_tab(0, cx));
        }),
        POISON,
        "owner drop→Copy in one update retires A before paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after owner drop cannot copy A"
    );
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    assert_eq!(select(cx, win, BODY), "SESSION_B_ISSUE_4");
    release.send(result);
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        "SESSION_B_ISSUE_4",
        "completion for a dropped owner cannot regain a Copy handler"
    );
    assert_eq!(cx.read(|cx| app.read(cx).active_session()), Some(owner_b));
    assert!(
        !cx.read(|cx| app.read(cx).ui.contains_key(&owner_a)),
        "retired session owns no surviving UI resources"
    );
    assert_eq!(
        before_b,
        repo_fingerprint(&repo_b),
        "session B repo changed"
    );
    finish(cx, fixture_a, app, win, before_a);
    drop(fixture_b);
}

pub fn scenario_issue_conversation_copy_priority(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish("a", "**BASE_SELECTION**", &[]);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "BASE_SELECTION");
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_reply_body_for_e2e(4, "reply-private-text", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(win, "secondary-a");
    assert_eq!(
        copy(cx, win),
        "reply-private-text",
        "real focused Reply Input copy has priority over selected conversation"
    );
    // Return focus through the product's existing selected-thread entry, then
    // physically select again; no SDK selection method is called by a test.
    load(cx, &app, win);
    assert_eq!(select(cx, win, BODY), "BASE_SELECTION");
    let head = cx.read(|cx| app.read(cx).view().rows[0].id.clone());
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
        }),
        POISON,
        "modal open→Copy in one update retires base conversation before modal paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after modal opening cannot copy the retired base"
    );
    measure(cx, win, "active-modal/create-branch");
    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .and_then(|modal| modal.input_state.clone())
        })
        .expect("the real branch modal creates its production Input");
    cx.update_window(win, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(win, "m o d a l");
    cx.run_until_parked();
    cx.simulate_keystrokes(win, "secondary-a");
    assert_eq!(
        copy(cx, win),
        "modal",
        "modal Input copy wins and excludes the retired base post"
    );
    cx.simulate_keystrokes(win, "escape");
    cx.run_until_parked();
    assert_eq!(
        copy(cx, win),
        POISON,
        "closing modal does not revive the old base selection"
    );
    assert_eq!(
        select(cx, win, BODY),
        "BASE_SELECTION",
        "new drag after modal retirement remains usable"
    );
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| app.show_graph_mode(cx));
        }),
        POISON,
        "mode departure→Copy in one update retires conversation before paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after mode departure cannot copy the retired conversation"
    );
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    cx.run_until_parked();
    assert_eq!(select(cx, win, BODY), "BASE_SELECTION");
    let back = measure(cx, win, "issue-thread-back").unwrap();
    // Queue only the ensuing list transport; the home transition is real.
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(
        kagi_domain::github::IssueListSnapshot {
            issues: Vec::new(),
            mentioned_numbers: Vec::new(),
            base_repo: BASE.into(),
            next_cursor: None,
        },
    )));
    assert_eq!(
        click_copy_before_paint(cx, win, back.center()),
        POISON,
        "real home click→Copy in one update retires conversation before paint"
    );
    assert_eq!(
        copy(cx, win),
        POISON,
        "Cmd-C after returning home cannot copy the retired conversation"
    );
    cx.run_until_parked();
    drop(input);
    finish(cx, fixture, app, win, before);
}
