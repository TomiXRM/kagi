//! #1702: async restore cannot consume unseen text, and a successful post
//! invalidates its shared input even after Graph discarded the PR mode.
use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_domain::github::{PullRequest, ReviewVerdict};

use crate::evidence_support::{deferred, pull_request};
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::recovery_operations::wait_idle;

const BASE: &str = "github.com/acme/widgets";
const TEXT: &str = "Keep this review body until it is actually submitted";

fn offline_gh(dir: &Path) -> OfflineGh {
    OfflineGh::with_script(&format!(
        r#"#!/bin/sh
case "$1 $2" in
  'pr review'|'pr comment')
    printf '%s\n' "$2" >> '{calls}'
    cat > '{body}'
    echo https://example.invalid/acme/widgets/pull/7#comment ;;
  'pr view') echo '{{"reviews":[],"comments":[]}}' ;;
  'pr list') echo '[]' ;;
  *) exit 1 ;;
esac
"#,
        calls = dir.join("writes").display(),
        body = dir.join("body").display(),
    ))
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    crate::presentation_support::paint(cx, window);
}

fn composer(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> String {
    cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    })
}

fn open(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pr: &PullRequest,
) {
    app.update(cx, |app, cx| app.pr_mode_open(pr, cx));
    wait_idle(cx, app);
    paint(cx, window);
}

fn pr() -> PullRequest {
    let mut pr = pull_request(7, "draft lifecycle", "main");
    pr.base_repo = BASE.to_owned();
    pr
}

fn drafts(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> kagi_git::drafts::PrCommentDraftSession {
    cx.read(|cx| {
        let app = app.read(cx);
        app.ui[&app.active_session().unwrap()]
            .pr_comment_drafts
            .clone()
            .expect("the session owns a durable draft slot")
    })
}

fn type_comment(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    let input = cx.read(|cx| app.read(cx).pr_comment_input.clone().unwrap());
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.replace(text.to_owned(), window, cx);
        });
    })
    .unwrap();
    paint(cx, window);
}

pub fn scenario_loading_blocks_review(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let state = tempfile::tempdir().unwrap();
    let _gh = offline_gh(state.path());
    {
        let saved = kagi_git::drafts::PrCommentDraftSession::new(&repo);
        saved.queue(BASE, 7, TEXT);
        kagi_git::drafts::flush_issue_drafts().unwrap();
    }
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred(cx);
    KagiApp::hold_next_pr_comment_restore_for_e2e(hold);
    open(cx, &app, window, &pr());
    cx.read(|cx| {
        let app = app.read(cx);
        let mode = app.pr_mode().unwrap();
        assert!(mode.tabs[mode.active.unwrap()].comment_loading);
        assert!(!app.app_sessions.has_leases());
    });
    // Exercise the actual dispatch entry, not only the button's disabled paint.
    app.update(cx, |app, cx| {
        app.start_pr_review(ReviewVerdict::Approve, cx)
    });
    wait_idle(cx, &app);
    assert!(
        !state.path().join("writes").exists(),
        "Approve must not send an empty body while the saved draft is unseen"
    );
    assert_eq!(
        drafts(cx, &app).load(BASE, 7).as_deref(),
        Some(TEXT),
        "a blocked approval must not consume the saved body"
    );
    release.send(());
    paint(cx, window);
    assert_eq!(composer(cx, &app), TEXT);
    app.update(cx, |app, cx| {
        app.start_pr_review(ReviewVerdict::Approve, cx)
    });
    wait_idle(cx, &app);
    paint(cx, window);
    assert_eq!(
        std::fs::read_to_string(state.path().join("body")).unwrap(),
        TEXT
    );
    assert_eq!(
        std::fs::read_to_string(state.path().join("writes")).unwrap(),
        "review\n"
    );
    assert!(drafts(cx, &app).load(BASE, 7).is_none());
    unmount(cx, app, window);
}

pub fn scenario_post_after_graph(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let state = tempfile::tempdir().unwrap();
    let _gh = offline_gh(state.path());
    let (app, window) = mount(cx, &repo);
    open(cx, &app, window, &pr());
    let input = cx.read(|cx| app.read(cx).pr_comment_input.clone().unwrap());
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.replace(TEXT.to_owned(), window, cx));
    })
    .unwrap();
    paint(cx, window);
    assert_eq!(composer(cx, &app), TEXT);
    let (hold, release) = deferred(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.start_pr_comment(cx);
        assert!(
            app.app_sessions.has_leases(),
            "the real post is held in flight"
        );
        app.show_graph_mode(cx);
        assert!(app.pr_mode().is_none(), "Graph drops the PR mode");
    });
    paint(cx, window);
    release.send(());
    wait_idle(cx, &app);
    paint(cx, window);
    assert_eq!(
        std::fs::read_to_string(state.path().join("body")).unwrap(),
        TEXT
    );
    assert!(drafts(cx, &app).load(BASE, 7).is_none());
    open(cx, &app, window, &pr());
    assert_eq!(
        composer(cx, &app),
        "",
        "reopening must not requeue the already-posted shared input as a new edit"
    );
    app.update(cx, |app, cx| app.start_pr_comment(cx));
    wait_idle(cx, &app);
    assert_eq!(
        std::fs::read_to_string(state.path().join("writes")).unwrap(),
        "comment\n"
    );
    assert!(drafts(cx, &app).load(BASE, 7).is_none());
    drop(input);
    unmount(cx, app, window);
}

pub fn scenario_same_clone_sessions(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let state = tempfile::tempdir().unwrap();
    let _gh = offline_gh(state.path());
    let (first, first_window) = mount(cx, &repo);
    let (second, second_window) = mount(cx, &repo);
    assert_ne!(
        cx.read(|cx| first.read(cx).active_session()),
        cx.read(|cx| second.read(cx).active_session()),
        "two windows attach independent SessionIds to the exact same clone"
    );
    open(cx, &first, first_window, &pr());
    type_comment(cx, &first, first_window, "first session body 日本語");
    kagi_git::drafts::flush_issue_drafts().unwrap();
    open(cx, &second, second_window, &pr());
    assert_eq!(
        composer(cx, &second),
        "",
        "restoring a second owner must not borrow the first owner's saved body"
    );
    type_comment(cx, &second, second_window, "second session body");
    first.update(cx, |app, cx| app.show_graph_mode(cx));
    open(cx, &first, first_window, &pr());
    assert_eq!(composer(cx, &first), "first session body 日本語");
    let posted_version = drafts(cx, &first).version(BASE, 7);
    type_comment(cx, &second, second_window, "second session newer body");
    assert_ne!(posted_version, drafts(cx, &second).version(BASE, 7));
    first.update(cx, |app, cx| app.start_pr_comment(cx));
    wait_idle(cx, &first);
    paint(cx, first_window);
    paint(cx, second_window);
    assert_eq!(
        std::fs::read_to_string(state.path().join("body")).unwrap(),
        "first session body 日本語",
        "the real recorded post uses only the first owner's text"
    );
    assert_eq!(composer(cx, &first), "", "the posted version clears");
    assert!(drafts(cx, &first).load(BASE, 7).is_none());
    assert_eq!(composer(cx, &second), "second session newer body");
    assert_eq!(
        drafts(cx, &second).load(BASE, 7).as_deref(),
        Some("second session newer body"),
        "the other owner's newer draft remains queued and recoverable"
    );
    second.update(cx, |app, cx| app.show_graph_mode(cx));
    open(cx, &second, second_window, &pr());
    assert_eq!(composer(cx, &second), "second session newer body");
    kagi_git::drafts::flush_issue_drafts().unwrap();
    assert_eq!(
        std::fs::read_to_string(state.path().join("writes")).unwrap(),
        "comment\n",
        "only the explicitly posted owner reaches the fixture transport"
    );
    unmount(cx, first, first_window);
    unmount(cx, second, second_window);
}
