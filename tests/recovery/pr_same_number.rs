//! #940 review: one session holds PR #7 of repository A and PR #7 of
//! repository B — a clone opened from Home while `gh repo set-default` names
//! the other repository. Every read lands on the tab of its own PR: the
//! conversation, the review threads and the merge status of A#7 on A's tab
//! and of B#7 on B's tab. Opening B#7 while the session's PR list holds A#7
//! requests B's details, not A's.
//!
//! The stand-in `gh` answers by the repository each call names and logs the
//! calls, so the test waits for the reads to finish and then looks at where
//! they landed.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the PR window");
}

/// The parked draft of `base_repo`'s #7.
fn draft(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, base_repo: &str) -> String {
    cx.read(|cx| {
        app.read(cx)
            .pr_mode()
            .and_then(|mode| {
                mode.tabs
                    .iter()
                    .find(|t| t.pr.number == 7 && t.pr.base_repo == base_repo)
            })
            .map(|t| t.comment_draft.clone())
            .expect("the tab")
    })
}

/// What the composer's box shows.
fn composer(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> String {
    cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    })
}

fn type_comment(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    let input = cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .clone()
            .expect("the composer's box exists once drawn")
    });
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.replace(text.to_owned(), window, cx);
        })
    })
    .expect("type into the real composer");
}

const A: &str = "github.com/acme/a";
const B: &str = "github.com/acme/b";

fn gh_script(log: &Path) -> String {
    format!(
        r#"#!/bin/sh
case "$*" in
  *"{A} "*|*"-f owner=acme -f name=a "*) who=a ;;
  *"{B} "*|*"-f owner=acme -f name=b "*) who=b ;;
  *) who=none ;;
esac
case "$1 $2 $3" in
  "pr view 7")
    echo "convo-$who" >> '{log}'
    echo '{{"reviews":[{{"author":{{"login":"'$who'-reviewer"}},"state":"APPROVED","body":"from '$who'","submittedAt":"2026-10-03T00:00:00Z"}}],"comments":[]}}' ;;
  "pr view -R")
    echo "detail-$who" >> '{log}'
    exit 1 ;;
  "api graphql "*)
    case "$*" in
      *mergeStateStatus*)
        echo "merge-$who" >> '{log}'
        echo '{{"data":{{"repository":{{"pullRequest":{{"id":"PR_'$who'","mergeStateStatus":"CLEAN","reviewThreads":{{"nodes":[]}},"mergeQueueEntry":null}}}}}}}}' ;;
      *reviewThreads*)
        echo "threads-$who" >> '{log}'
        echo '{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"path":"'$who'.rs","line":1,"startLine":null,"originalLine":1,"diffSide":"RIGHT","isOutdated":false,"isResolved":false,"viewerCanResolve":false,"comments":{{"nodes":[{{"databaseId":1,"author":{{"login":"'$who'-threader"}},"body":"x","createdAt":"2026-10-03T00:00:00Z","diffHunk":"","replyTo":null}}]}}}}]}}}}}}}}}}' ;;
      *) exit 1 ;;
    esac ;;
  *) exit 1 ;;
esac
"#,
        log = log.display()
    )
}

fn calls(log: &Path) -> Vec<String> {
    let mut calls: Vec<String> = std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    calls.sort();
    calls
}

/// Wait until both PRs' conversation, threads and merge status were read.
fn wait_reads(cx: &mut VisualTestAppContext, log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let done = calls(log)
            .iter()
            .filter(|c| !c.starts_with("detail-"))
            .count();
        if done >= 6 {
            cx.run_until_parked();
            return;
        }
        assert!(Instant::now() < deadline, "the PR reads never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What landed on the tab of `base_repo`'s #7: (review authors, merge status
/// node id, authors of the line comments built from its review threads).
fn landed(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    base_repo: &str,
) -> (Vec<String>, Option<String>, Vec<String>) {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode
            .tabs
            .iter()
            .find(|t| t.pr.number == 7 && t.pr.base_repo == base_repo)
            .unwrap_or_else(|| panic!("a tab for {base_repo}#7"));
        (
            tab.reviews.iter().map(|r| r.author.clone()).collect(),
            tab.merge_status.as_ref().map(|s| s.node_id.clone()),
            tab.line_comments.iter().map(|c| c.author.clone()).collect(),
        )
    })
}

pub fn scenario_pr_same_number(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("fixture path");
    let state = tempfile::tempdir().unwrap();
    let log = state.path().join("calls");
    let _gh = OfflineGh::with_script(&gh_script(&log));
    // This boundary needs successful local PR loads: a failed ref fetch
    // legitimately starts a newer shared-list read and supersedes a held one.
    // Keep the GitHub identities raw; only their private fixture transport is
    // rewritten, as in pr_viewed.
    let remotes = tempfile::tempdir().unwrap();
    let remote_a = remotes.path().join("a.git");
    let remote_b = remotes.path().join("b.git");
    for (name, base_repo, remote) in [("a", A, &remote_a), ("b", B, &remote_b)] {
        git(
            remotes.path(),
            &["init", "-q", "--bare", &format!("{name}.git")],
        );
        git(
            &repo,
            &[
                "push",
                "-q",
                remote.to_str().unwrap(),
                "main:refs/heads/main",
            ],
        );
        let url = format!("https://{base_repo}.git");
        git(&repo, &["remote", "add", name, &url]);
        let file_url = format!("file://{}", remote.display());
        git(
            &repo,
            &["config", &format!("url.{file_url}.insteadOf"), &url],
        );
    }
    let head_a =
        crate::pr_viewed::push_pr_head(&repo, &remote_a, "main", &[("a.rs", "A's PR content\n")]);
    let head_b =
        crate::pr_viewed::push_pr_head(&repo, &remote_b, "main", &[("b.rs", "B's PR content\n")]);
    let (app, window) = mount(cx, &repo);

    let pr = |base_repo: &str, head: &str| {
        let mut pr = pull_request(7, &format!("{base_repo} #7"), "fix");
        pr.base_repo = base_repo.to_string();
        pr.url = format!("https://{base_repo}/pull/7");
        pr.head_sha = head.to_owned();
        pr
    };
    let (a7, b7) = (pr(A, &head_a), pr(B, &head_b));
    // The session's list is A's (its `gh repo set-default`).
    app.update(cx, |app, _| {
        app.ui_mut().expect("a repository session").github_prs = vec![a7.clone()];
    });
    // Admit the real Git fetches serially. The existing wait_loaded helper
    // pumps completions but does not advance the TestDispatcher's retry timer
    // for a second fetch behind a writer; that admission is not this boundary.
    app.update(cx, |app, cx| app.pr_mode_open(&a7, cx));
    crate::pr_viewed::wait_loaded(cx, &app, &head_a);
    app.update(cx, |app, cx| app.pr_mode_open(&b7, cx));
    crate::pr_viewed::wait_loaded(cx, &app, &head_b);
    wait_reads(cx, &log);

    let (a_reviews, a_merge, a_threads) = landed(cx, &app, A);
    let (b_reviews, b_merge, b_threads) = landed(cx, &app, B);
    assert_eq!(
        (a_reviews, a_merge.as_deref(), a_threads),
        (
            vec!["a-reviewer".to_string()],
            Some("PR_a"),
            vec!["a-threader".to_string()]
        ),
        "A#7's tab holds A's conversation, merge status and threads"
    );
    assert_eq!(
        (b_reviews, b_merge.as_deref(), b_threads),
        (
            vec!["b-reviewer".to_string()],
            Some("PR_b"),
            vec!["b-threader".to_string()]
        ),
        "B#7's tab holds B's conversation, merge status and threads"
    );
    let calls = calls(&log);
    assert!(
        calls.iter().any(|c| c == "detail-b"),
        "opening B#7 reads B's details although the list holds A#7: {calls:?}"
    );

    // #940 review: the one composer follows the PR by repository and number.
    // A draft typed on B#7 (open, as it was opened last) is not carried to
    // A#7 when A's tab comes forward, and comes back with B's tab.
    paint(cx, window);
    type_comment(cx, &app, window, "for B");
    paint(cx, window);
    app.update(cx, |app, cx| app.pr_mode_open(&a7, cx));
    paint(cx, window);
    assert_eq!(
        (draft(cx, &app, A), draft(cx, &app, B), composer(cx, &app)),
        (String::new(), "for B".to_string(), String::new()),
        "B#7's draft stays B's; A#7 opens with its own (empty) box"
    );
    app.update(cx, |app, cx| app.pr_mode_open(&b7, cx));
    paint(cx, window);
    assert_eq!(
        composer(cx, &app),
        "for B",
        "B's draft comes back with B's tab"
    );

    // A successful list refresh replaces the PR facts, not either PR's
    // composer. Hold it across a PR switch so B's accepted background
    // update cannot overwrite the live box now belonging to A.
    let (refresh, answer) = crate::evidence_support::deferred(cx);
    kagi::ui::e2e::queue_github_pr_fetch(refresh);
    let refresh_generation = app.update(cx, |app, cx| {
        app.refresh_github_prs(cx);
        assert!(app.ui().github_prs_loading, "the held read is admitted");
        app.ui().github_prs_gen
    });
    app.update(cx, |app, cx| app.pr_mode_open(&a7, cx));
    paint(cx, window);
    let a_text = "for A\n日本語\n";
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let input = app.pr_comment_input.clone().expect("A's composer");
            input.update(cx, |state, cx| state.replace(a_text.to_owned(), window, cx));
        });
    })
    .unwrap();
    paint(cx, window);
    cx.read(|cx| {
        let ui = app.read(cx).ui();
        assert_eq!(
            ui.github_prs_gen, refresh_generation,
            "the held producer must not be superseded before delivery"
        );
        assert!(ui.github_prs_loading, "the producer is still held");
    });
    let mut refreshed_b = b7.clone();
    refreshed_b.title = "B refreshed while A composes".into();
    answer.send(Ok(vec![a7.clone(), refreshed_b]));
    paint(cx, window);
    cx.read(|cx| {
        let app = app.read(cx);
        let mode = app.pr_mode().expect("PR mode after refresh");
        assert_eq!(mode.tabs[mode.active.unwrap()].pr.key(), a7.key());
        let ui = app.ui();
        assert_eq!(ui.github_prs_gen, refresh_generation);
        assert!(!ui.github_prs_loading, "the held completion was accepted");
        assert!(ui.github_error.is_none(), "the refresh succeeded");
        let b = ui.github_prs.iter().find(|pr| pr.is(&b7.key())).unwrap();
        assert_eq!(
            b.title, "B refreshed while A composes",
            "the accepted Open collection carries the distinct refreshed title"
        );
    });
    assert_eq!(
        (draft(cx, &app, A), draft(cx, &app, B), composer(cx, &app)),
        (a_text.to_owned(), "for B".to_owned(), a_text.to_owned()),
        "accepted background PR facts must preserve both drafts and A's live box"
    );

    // Read the title through the actual dashboard renderer as well as the
    // accepted collection. Open-tab facts may follow a different strip; they
    // are not an observer of shared Open-list acceptance.
    app.update(cx, |app, cx| app.pr_mode_home(cx));
    kagi::ui::list_a11y::clear_recorded_lists();
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "pr-home-title-7");
    paint(cx, window);
    paint(cx, window);
    let list = kagi::ui::list_a11y::recorded_list("pr-list")
        .expect("the accepted PR collection is rendered as a list");
    assert!(
        list.rows
            .values()
            .any(|(label, _)| { label.contains("B refreshed while A composes") }),
        "a freshly drawn PR row exposes the distinct refreshed title"
    );
    let title = kagi::ui::e2e::control_bounds(window.window_id(), "pr-home-title-7")
        .expect("the PR title cell is drawn");
    assert!(title.size.width > gpui::px(0.) && title.size.height > gpui::px(0.));
    app.update(cx, |app, cx| app.pr_mode_open(&a7, cx));
    paint(cx, window);
    assert_eq!(
        composer(cx, &app),
        a_text,
        "A's box survives list inspection"
    );

    // The repository's actual snapshot reload also keeps the open PR state.
    app.update(cx, |app, cx| app.reload(cx));
    paint(cx, window);
    assert_eq!(
        composer(cx, &app),
        a_text,
        "reload preserves A's live draft"
    );
    app.update(cx, |app, cx| app.pr_mode_open(&b7, cx));
    paint(cx, window);
    assert_eq!(
        (draft(cx, &app, A), composer(cx, &app)),
        (a_text.to_owned(), "for B".to_owned()),
        "reload preserves the parked draft as well as the active draft"
    );

    // The refresh/reload must preserve the conversation the production gh
    // subprocess and parser delivered, not just the composer. Draw its real
    // review anchor and first row on each PR after restoring that PR's box.
    for (pr, author) in [(&a7, "a-reviewer"), (&b7, "b-reviewer")] {
        app.update(cx, |app, cx| {
            app.pr_mode_open(pr, cx);
            app.pr_mode_show(kagi::ui::pr_mode::PrView::Review, cx);
        });
        for control in ["pr-feed-review", "pr-feed-entry-0"] {
            kagi::ui::e2e::clear_control_bounds(window.window_id(), control);
        }
        paint(cx, window);
        paint(cx, window);
        cx.read(|cx| {
            let mode = app.read(cx).pr_mode().expect("restored PR page");
            let tab = &mode.tabs[mode.active.unwrap()];
            assert_eq!(tab.pr.key(), pr.key());
            assert!(tab.conversation_loaded);
            assert_eq!(
                (
                    tab.reviews.len(),
                    tab.comments.len(),
                    tab.line_comments.len()
                ),
                (1, 0, 1),
                "refresh/reload must retain the exact conversation counts"
            );
            assert_eq!(tab.feed_entries.len(), 2);
            assert_eq!(tab.reviews[0].author, author);
            assert_eq!(
                tab.reviews[0].body,
                if pr.base_repo == A {
                    "from a"
                } else {
                    "from b"
                }
            );
            assert_eq!(tab.line_comments[0].body, "x");
        });
        let heading = kagi::ui::e2e::control_bounds(window.window_id(), "pr-feed-review")
            .expect("the restored review heading is drawn");
        let entry = kagi::ui::e2e::control_bounds(window.window_id(), "pr-feed-entry-0")
            .expect("the restored conversation row is drawn");
        assert!(
            entry.top() >= heading.top(),
            "the row follows its review heading"
        );
        assert_eq!(
            composer(cx, &app),
            if pr.base_repo == A { a_text } else { "for B" },
            "the rendered conversation and composer must have the same PR owner"
        );
    }
    // The exact same PrKey in a second repository session is a separate
    // composer owner. Exercise real visible text, not a seeded parked draft.
    let first_owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    type_comment(cx, &app, window, "first session B");
    paint(cx, window);
    assert_eq!(composer(cx, &app), "first session B");
    assert_eq!(draft(cx, &app, B), "first session B");
    let other_fixture = build_fixture();
    let other_repo = other_fixture.path().canonicalize().unwrap();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx));
    });
    paint(cx, window);
    let other_owner = cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pr_mode().is_none());
        app.active_session().unwrap()
    });
    assert_ne!(first_owner, other_owner);
    app.update(cx, |app, cx| app.pr_mode_open(&b7, cx));
    paint(cx, window);
    assert_eq!(
        composer(cx, &app),
        "",
        "the other session owns an empty draft"
    );
    type_comment(cx, &app, window, "other session B");
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    paint(cx, window);
    assert_eq!(
        composer(cx, &app),
        "first session B",
        "the identical PR key must restore the returning session's draft"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let parked = app.ui[&other_owner].pr_mode.as_ref().unwrap();
        assert_eq!(parked.tabs[0].comment_draft, "other session B");
    });

    app.update(cx, |app, cx| app.switch_repo(1, cx));
    paint(cx, window);
    assert_eq!(composer(cx, &app), "other session B");

    // A closed-and-reopened repository is a new session, not permission to
    // carry the old session's identical PR-key draft into a new tab.
    app.update(cx, |app, cx| app.close_tab(1, cx));
    paint(cx, window);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx));
        assert_ne!(app.active_session(), Some(other_owner));
        app.pr_mode_open(&b7, cx);
    });
    paint(cx, window);
    assert_eq!(composer(cx, &app), "");
    assert_eq!(draft(cx, &app, B), "");

    unmount(cx, app, window);
}
