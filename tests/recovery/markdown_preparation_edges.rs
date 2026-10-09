//! Consumer-visible invalidation of the shared GitHub Markdown preparation.
//! Keep one selected Issue, window and Markdown element ID across every frame.
use super::{drag_and_copy, measure};
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::i18n::{self, Lang};

const BODY_ID: &str = "issue-thread-body-4-md";
const OLD_BODY: &str = "**CACHE_OLD_SENTINEL** and ~~OLD_GFM~~.";
const NEW_BODY: &str = "**CACHE_NEW_SENTINEL** and ~~NEW_GFM~~.";
const OLD_RENDERED: &str = "CACHE_OLD_SENTINEL and OLD_GFM.";
const NEW_RENDERED: &str = "CACHE_NEW_SENTINEL and NEW_GFM.";

// SavedKeys restores persisted settings; this also restores the process-global
// language if an assertion unwinds before the normal app/menu restoration.
struct RestoreLanguage(Lang);

impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        i18n::set_lang(self.0);
    }
}

fn copy_body(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> String {
    cx.run_until_parked();
    let bounds = measure(cx, win, BODY_ID).expect("the same Issue Markdown body must be drawn");
    let viewport = cx
        .update_window(win, |_, window, _| window.viewport_size())
        .unwrap();
    assert!(
        bounds.left() >= gpui::px(0.)
            && bounds.top() >= gpui::px(0.)
            && bounds.right() <= viewport.width
            && bounds.bottom() <= viewport.height,
        "the entire selected body must be painted inside the native viewport"
    );
    // The inherited helper poisons only VisualTestPlatform's private clipboard,
    // then sends mouse down/move/up and secondary-c through the real window.
    drag_and_copy(cx, win, bounds)
}

fn assert_copy(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    expected: &str,
    forbidden: &str,
) {
    let copied = copy_body(cx, win);
    assert_eq!(
        copied.trim(),
        expected,
        "physical selection must copy exactly the currently rendered Markdown text"
    );
    assert!(
        !copied.contains(forbidden),
        "the private clipboard must exclude the prior rendered text: {copied:?}"
    );
}

pub fn scenario_markdown_preparation_edges(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_lang = i18n::lang();
    let _restore_language = RestoreLanguage(original_lang);
    i18n::set_lang(Lang::En);
    let en_placeholder = kagi::ui::i18n::Msg::IssueNoDescription.t();
    i18n::set_lang(Lang::Ja);
    let ja_placeholder = kagi::ui::i18n::Msg::IssueNoDescription.t();
    i18n::set_lang(original_lang);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(Lang::En, cx);
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
        app.seed_issue_navigation_for_e2e(cx);
        app.seed_issue_reply_for_e2e(4, cx);
        app.set_issue_body_for_e2e(4, OLD_BODY, cx);
    });

    // These different GFM sources intentionally have identical byte lengths.
    // Painting and copying establishes the same live consumer before replacing
    // its body, without remounting/reseeding its owner or ID.
    assert_eq!(OLD_BODY.len(), NEW_BODY.len());
    assert_copy(cx, win, OLD_RENDERED, "CACHE_NEW_SENTINEL");
    app.update(cx, |app, cx| app.set_issue_body_for_e2e(4, NEW_BODY, cx));
    assert_copy(cx, win, NEW_RENDERED, "CACHE_OLD_SENTINEL");

    // Set the raw body to empty only once. Subsequent frames change solely the
    // localized formatter placeholder, so raw-only invalidation would be stale.
    app.update(cx, |app, cx| app.set_issue_body_for_e2e(4, "", cx));
    assert_copy(cx, win, en_placeholder, "CACHE_NEW_SENTINEL");
    app.update(cx, |app, cx| app.set_lang(Lang::Ja, cx));
    assert_copy(cx, win, ja_placeholder, en_placeholder);
    app.update(cx, |app, cx| app.set_lang(Lang::En, cx));
    assert_copy(cx, win, en_placeholder, ja_placeholder);

    app.update(cx, |app, cx| app.set_lang(original_lang, cx));
    assert_eq!(before, repo_fingerprint(&repo), "repo mutated");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS markdown_preparation_edges");
}
