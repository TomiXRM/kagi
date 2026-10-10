//! #1091 consumer regressions. These scenarios are intentionally identical on
//! original pinned SDK / all-post Before and managed virtual-conversation After.
#[path = "issue_conversation_support.rs"]
pub(crate) mod support;
use gpui::{point, px, VisualTestAppContext};
use kagi::ui::theme;
use support::*;

const ALPHABET: &str = "A\n\nB\n\nC\n\nD\n\nE\n\nF\n\nG\n\nH\n\nI\n\nJ\n\nK\n\nL\n\nM\n\nN\n\nO\n\nP\n\nQ\n\nR\n\nS\n\nT\n\nU\n\nV\n\nW\n\nX\n\nY\n\nZ";

fn alphabet(producer: &Producer) {
    let owned: Vec<_> = (b'B'..=b'Z')
        .map(|letter| {
            (
                format!("IC_conversation_opaque_{letter}"),
                format!("**{}**", letter as char),
            )
        })
        .collect();
    let posts: Vec<_> = owned
        .iter()
        .map(|(id, text)| (id.as_str(), text.as_str()))
        .collect();
    producer.publish("a", "**A**", &posts);
}

pub fn scenario_issue_conversation_offscreen_copy(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    alphabet(&producer);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    let first = visible(cx, win, BODY).expect("initial A is physically visible");
    assert!(
        visible(cx, win, &comment(11)).is_none(),
        "M starts outside the viewport"
    );
    assert!(
        visible(cx, win, &comment(24)).is_none(),
        "Z starts outside the viewport"
    );
    down(cx, win, start(first));
    // One wheel jump: no intermediary M frame is requested by this consumer.
    wheel(cx, win, -100_000.);
    let last = seek(cx, win, &comment(24), -1.);
    assert_eq!(move_copy_before_paint(cx, win, end(last)), ALPHABET, "forward mousemove→Copy in one update must include every rendered offscreen post before paint");
    assert_eq!(copy(cx, win), ALPHABET, "physical forward drag and Cmd-C must exclude painted-fragment truncation/chrome/raw Markdown");
    up(cx, win, end(last));
    down(cx, win, end(last));
    wheel(cx, win, 100_000.);
    let first = seek(cx, win, BODY, 1.);
    assert_eq!(move_copy_before_paint(cx, win, start(first)), ALPHABET, "reverse mousemove→Copy in one update must have the exact current rendered projection before paint");
    assert_eq!(
        copy(cx, win),
        ALPHABET,
        "physical reverse selection and Cmd-C must have the same exact current rendered projection"
    );
    up(cx, win, start(first));
    assert_eq!(
        producer.requests(),
        vec!["a"],
        "one actual producer request accepted"
    );
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_giant_copy(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    // Independent rendered oracle: source carries bold syntax and blank source
    // lines; canonical rendered paragraphs carry no syntax and one LF each.
    let source = (0..180)
        .map(|line| format!("**GIANT_{line:03}**"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let expected = (0..180)
        .map(|line| format!("GIANT_{line:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    producer.publish("a", &source, &[]);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    let viewport = pane(cx, win);
    let bounds = measure(cx, win, BODY).expect("giant Markdown is drawn");
    assert!(
        bounds.bottom() > viewport.bottom(),
        "one giant body really is clipped by the viewport"
    );
    assert!(
        bounds.top() > viewport.top(),
        "first paragraph starts inside viewport"
    );
    down(cx, win, start(bounds));
    wheel(cx, win, -100_000.);
    let bounds = measure(cx, win, BODY).expect("last giant continuation remains addressable");
    let viewport = pane(cx, win);
    assert!(
        bounds.bottom() <= viewport.bottom() && bounds.bottom() > viewport.top(),
        "the actual final continuation must be reachable, not a truncated body"
    );
    let endpoint = end(bounds);
    assert_eq!(move_copy_before_paint(cx, win, endpoint), expected, "giant continuation mousemove→Copy in one update includes the full rendered range before paint");
    assert_eq!(
        copy(cx, win),
        expected,
        "complete clipped giant Copy must include all 180 rendered paragraphs through GIANT_179"
    );
    up(cx, win, endpoint);
    assert!(
        visible(cx, win, "issue-reply-composer").is_some(),
        "the end Reply composer shares the reachable scroll surface"
    );
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_accepted_identity(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish(
        "a",
        "**ROOT**",
        &[("IC_true_A", "**OLD_A**"), ("IC_true_B", "**OLD_B**")],
    );
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    assert_eq!(select(cx, win, &comment(1)), "OLD_B");
    producer.publish(
        "a",
        "**ROOT**",
        &[("IC_true_B", "**OLD_B**"), ("IC_true_A", "**OLD_A**")],
    );
    load(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        "OLD_B",
        "equal-author/equal-time reorder must retain the selected true B, not its prior index"
    );
    assert_eq!(
        select(cx, win, &comment(0)),
        "OLD_B",
        "new rendered order starts with true B"
    );
    producer.publish(
        "a",
        "**ROOT**",
        &[
            ("IC_true_P", "**NEW_P**"),
            ("IC_true_B", "**OLD_B**"),
            ("IC_true_A", "**OLD_A**"),
        ],
    );
    load(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        "OLD_B",
        "prepending duplicate metadata must not retarget live selection"
    );
    producer.publish(
        "a",
        "**ROOT**",
        &[
            ("IC_true_P", "**NEW_P**"),
            ("IC_true_B", "**NEW_B**"),
            ("IC_true_A", "**OLD_A**"),
        ],
    );
    load(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        POISON,
        "accepted same-byte mutation retires the old selected revision synchronously"
    );
    assert_eq!(
        select(cx, win, &comment(1)),
        "NEW_B",
        "fresh drag copies the accepted equal-byte replacement"
    );
    producer.publish(
        "a",
        "**ROOT**",
        &[("IC_true_P", "**NEW_P**"), ("IC_true_A", "**OLD_A**")],
    );
    load(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        POISON,
        "deleting selected true B cannot copy stale B or newly shifted A"
    );
    assert_eq!(
        select(cx, win, &comment(1)),
        "OLD_A",
        "surviving A remains usable after deletion"
    );
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_refresh_anchor(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    alphabet(&producer);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    let middle = seek(cx, win, &comment(11), -1.);
    let y = middle.top();
    load(cx, &app, win);
    assert_eq!(
        measure(cx, win, &comment(11)).unwrap().top(),
        y,
        "unchanged refresh preserves post and intra-post pixel anchor"
    );
    let owned: Vec<_> = (b'B'..=b'Z')
        .map(|letter| {
            (
                format!("IC_conversation_opaque_{letter}"),
                format!("**{}**", letter as char),
            )
        })
        .collect();
    let mut posts = vec![("IC_true_prepend", "**PREPEND**")];
    posts.extend(owned.iter().map(|(id, text)| (id.as_str(), text.as_str())));
    producer.publish("a", "**A**", &posts);
    load(cx, &app, win);
    assert_eq!(
        measure(cx, win, &comment(12)).unwrap().top(),
        y,
        "true M remains at the same viewport pixel after prepend"
    );
    producer.publish("a", "**A**", &posts[2..]);
    load(cx, &app, win);
    assert_eq!(
        measure(cx, win, &comment(10)).unwrap().top(),
        y,
        "deleting earlier true posts preserves M's pixel anchor"
    );
    // Select M physically; beginning and failing a refresh cannot overwrite it.
    assert_eq!(select(cx, win, &comment(10)), "M");
    producer.fail("a");
    load(cx, &app, win);
    assert_eq!(
        copy(cx, win),
        "M",
        "refresh failure retains accepted current selection"
    );
    assert_eq!(
        measure(cx, win, &comment(10)).unwrap().top(),
        y,
        "refresh error chrome must not displace the accepted scroll anchor"
    );
    assert!(
        measure(cx, win, "issue-mode-detail-error").is_some(),
        "real producer failure is visible, not hidden by a successful fake"
    );
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_geometry(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    producer.publish("a", "**Geometry αβ 日本語** and `current code`.", &[]);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    const EXPECTED: &str = "Geometry αβ 日本語 and \u{2009}current code\u{2009}.";
    assert_eq!(select(cx, win, BODY), EXPECTED);
    // Changing sidebar width changes the real remaining conversation width in
    // one native window; VisualTestAppContext cannot drain AppKit native resize.
    for (sidebar, zoom, slug) in [(340., 1.25, "apple-light"), (220., 1., "tokyo-night")] {
        app.update(cx, |app, cx| {
            app.sidebar.width = sidebar;
            theme::set_zoom(zoom);
            app.set_theme(slug, cx);
            cx.notify();
        });
        measure(cx, win, BODY).expect("same post is remeasured");
        assert_eq!(
            copy(cx, win),
            EXPECTED,
            "geometry-only width/zoom/theme changes preserve logical endpoints"
        );
        assert_eq!(
            select(cx, win, BODY),
            EXPECTED,
            "fresh clicks use new bounds, not stale pre-style geometry"
        );
    }
    finish(cx, fixture, app, win, before);
}

pub fn scenario_issue_conversation_edge_drag(cx: &mut VisualTestAppContext) {
    let _restore = Restore::capture();
    let producer = Producer::install();
    alphabet(&producer);
    let (fixture, app, win, before) = fixture(cx, BASE);
    load(cx, &app, win);
    let first = visible(cx, win, BODY).unwrap();
    let viewport = pane(cx, win);
    let edge = point(viewport.right() - px(12.), viewport.bottom() - px(2.));
    down(cx, win, start(first));
    move_to(cx, win, edge);
    for _ in 0..240 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
        if visible(cx, win, &comment(11)).is_some() {
            break;
        }
    }
    let middle = visible(cx, win, &comment(11)).expect("held edge must reach M");
    assert!(
        visible(cx, win, &comment(24)).is_none(),
        "cancellation is tested with Z still below, not at a clamped scroll end"
    );
    up(cx, win, edge);
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    assert_eq!(
        measure(cx, win, &comment(11)).unwrap().top(),
        middle.top(),
        "mouse-up at a held edge stops scrolling while more posts remain"
    );
    wheel(cx, win, 100_000.);
    let first = visible(cx, win, BODY).unwrap();
    down(cx, win, start(first));
    move_to(cx, win, edge);
    // Deterministic dispatcher ticks drive the SDK's actual held-edge task;
    // there are no sleeps and no elapsed-time/performance claim.
    for _ in 0..240 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
        if visible(cx, win, &comment(24)).is_some() {
            break;
        }
    }
    let last = visible(cx, win, &comment(24)).expect(
        "held pointer at outer edge must auto-scroll to never-painted Z without wheel input",
    );
    assert_eq!(move_copy_before_paint(cx, win, end(last)), ALPHABET, "held-edge re-hit mousemove→Copy in one update resolves the complete logical range before paint");
    assert_eq!(
        copy(cx, win),
        ALPHABET,
        "stationary edge ticks must re-hit logical selection as new posts enter"
    );
    up(cx, win, end(last));
    wheel(cx, win, 100_000.);
    let first = visible(cx, win, BODY).unwrap();
    down(cx, win, start(first));
    move_to(cx, win, edge);
    assert_eq!(
        copy_before_paint(cx, win, |_, cx| {
            app.update(cx, |app, cx| app.show_graph_mode(cx));
        }),
        POISON,
        "held-drag scope departure→Copy in one update retires selection before paint"
    );
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    assert_eq!(
        copy(cx, win),
        POISON,
        "scope departure retires held selection and its edge callback"
    );
    up(cx, win, edge);
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    assert_eq!(
        visible(cx, win, BODY).unwrap().top(),
        first.top(),
        "retired edge callback cannot scroll inactive owner's list"
    );
    finish(cx, fixture, app, win, before);
}
