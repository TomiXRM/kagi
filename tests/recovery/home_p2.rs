//! Home's display fixes left over from #960's review (#968), Tier A: a
//! review-request avatar found whatever case its Enterprise host is spelt
//! in, and one clone-card redraw ticker however quickly the card is
//! reopened. (The tab lists' arrowed-to cell is in `keyboard_nav`.)
use std::time::{Duration, Instant};

use gpui::{Entity, VisualTestAppContext};
use kagi::ui::home_github::CloneModal;
use kagi::ui::{avatar_fetch, e2e, KagiApp};
use kagi_domain::github_repos::RepoListing;
use kagi_git::github_repos_cache::{WorkItem, WorkList, WorkLists};

use crate::macos::{build_fixture, mount, unmount};

/// Let `wait` pass on the test dispatcher's clock, which the ticker's timer
/// runs on, running what becomes due on the way.
fn pump(cx: &mut VisualTestAppContext, wait: Duration) {
    let step = Duration::from_millis(100);
    let mut passed = Duration::ZERO;
    while passed < wait {
        cx.advance_clock(step);
        cx.run_until_parked();
        passed += step;
    }
}

/// A 1×1 PNG, the bytes an avatar request answers with.
fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(1, 1)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode a PNG");
    bytes.into_inner()
}

/// An Enterprise reviewer whose host the search spells in capitals: the
/// avatar is fetched (from the lower-cased host) and stored, and the row
/// must find it under the same key, not draw the initials (#960 review).
pub fn scenario_home_review_avatar_host(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let item = WorkItem {
        host: "GHE.example.com".into(),
        name_with_owner: "acme/widgets".into(),
        number: 5,
        title: "Please review".into(),
        url: "https://GHE.example.com/acme/widgets/pull/5".into(),
        is_draft: false,
        author: "octo".into(),
        updated_at: String::new(),
    };
    // The answer to the avatar request, in the fetcher's disk cache (no
    // network in Tier A).
    let url = avatar_fetch::avatar_url_for_login(Some("ghe.example.com"), "octo");
    let cached = avatar_fetch::cache_path_for_url(&url).expect("the avatar cache");
    std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
    std::fs::write(&cached, png()).unwrap();

    app.update(cx, |app, cx| {
        app.home_github.work.lists = Some(WorkLists {
            review_requests: WorkList {
                items: vec![item.clone()],
                truncated: false,
            },
            ..WorkLists::default()
        });
        e2e::ensure_home_avatars(app, cx);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while cx.read(|cx| app.read(cx).avatars.images.is_empty()) {
        assert!(Instant::now() < deadline, "the avatar was never fetched");
        pump(cx, Duration::from_millis(100));
    }
    assert!(
        cx.read(|cx| e2e::home_review_avatar_shown(app.read(cx), &item)),
        "the row finds the avatar stored for its host, whatever its case"
    );

    std::fs::remove_file(&cached).unwrap();
    app.update(cx, |app, _| app.home_github.work.lists = None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS home_review_avatar_host");
}

/// Wait until no clone-card ticker runs: each ends at its next wake.
fn tickers_gone(cx: &mut VisualTestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while e2e::clone_tickers() > 0 {
        assert!(Instant::now() < deadline, "a clone-card ticker never ended");
        pump(cx, Duration::from_millis(100));
    }
}

/// Closing the running clone's card and bringing it back within the
/// ticker's second starts a new ticker while the old one sleeps: the old one
/// must end when it wakes, so one ticker redraws the card (#960 review).
pub fn scenario_clone_card_ticker(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    tickers_gone(cx);
    let listing = RepoListing {
        name_with_owner: "acme/widgets".into(),
        host: "github.com".into(),
        is_fork: false,
        is_private: false,
        description: String::new(),
        updated_at: String::new(),
    };
    let reopen = |cx: &mut VisualTestAppContext, app: &Entity<KagiApp>| {
        app.update(cx, |app, cx| app.home_github_pick(listing.clone(), cx));
    };
    // A clone running, its card off screen; its row brings the card back.
    app.update(cx, |app, _| {
        app.home_github.cloning = Some(CloneModal {
            listing: listing.clone(),
            target: None,
            started: Some(Instant::now()),
        });
    });
    reopen(cx, &app);
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_some()));
    assert_eq!(e2e::clone_tickers(), 1, "the card's ticker runs");
    app.update(cx, |app, _| app.clear_clone_modal());
    reopen(cx, &app);
    assert_eq!(
        e2e::clone_tickers(),
        2,
        "precondition: reopened while the first ticker sleeps"
    );
    pump(cx, Duration::from_millis(1600));
    assert_eq!(
        e2e::clone_tickers(),
        1,
        "the older ticker ended at its wake; one redraws the card"
    );

    app.update(cx, |app, _| {
        app.clear_clone_modal();
        app.home_github.cloning = None;
    });
    tickers_gone(cx);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS clone_card_ticker");
}
