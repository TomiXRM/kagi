//! PR conversation rendering: the description card, the review/comment
//! timeline, and the inline `diff_hunk` suggestion blocks.
//!
//! Split out of `pr_mode.rs` — these are pure renderers over a `PrTab`, with
//! no state of their own.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{Comment, PullRequest, Review, ReviewComment};

use super::i18n::Msg;
use super::pr_mode::{card_bg, card_border, card_pane_bg, ci_glyph};
// issue #414: `@login` handles come straight from `gh` JSON — sanitize before render.
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::types::ToastKind;
use super::KagiApp;

/// The PR description as rendered markdown - the card alone, with no scroll
/// container of its own: it is the first section of the feed (ADR-0200).
fn description_card(pr: &PullRequest, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    use gpui_component::text::{TextView, TextViewStyle};
    use gpui_component::ActiveTheme as _;
    let body = if pr.body.trim().is_empty() {
        format!("_{}_", Msg::PrModeNoDescription.t())
    } else {
        // GitHub bodies are CRLF, wrap inline code across lines and carry bot
        // HTML footers — all of which crash gpui-component's inline layouter
        // ("text argument should not contain newlines"). Normalise first.
        // Thin-space padding inside `code` spans (the renderer paints the bare
        // glyph range) — same trick as the Editor's markdown preview.
        kagi_ui_editor::markdown::pad_inline_code(
            &kagi_domain::message::sanitize_markdown_for_view(&pr.body),
        )
    };
    // Table borders: gpui-component draws them in `theme().border`, which
    // kagi maps to the near-background `selected` — invisible on the card.
    // The style refinements override the container + cell borders.
    let mut table = gpui::StyleRefinement::default();
    let mut table_cell = gpui::StyleRefinement::default();
    let mut grid: gpui::Hsla = rgb(theme().text_muted).into();
    grid.a = 0.95;
    table.border_color = Some(grid);
    table_cell.border_color = Some(grid);
    let style = TextViewStyle {
        heading_base_font_size: theme::scaled_px(17.),
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        table,
        table_cell,
        ..Default::default()
    };
    let (g, c) = ci_glyph(pr.ci);
    // A reading card: rounded surface floating on the base background, a
    // measure-limited column and its own small header — visually a document,
    // not another list, so the boundary with the commit strip is obvious.
    let card_header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pb_2()
        .mb_3()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrModeDescription.t())),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(safe_text(&format!("@{}", pr.author))),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(c))
                .child(SharedString::from(g)),
        )
        .when(pr.is_draft, |el| {
            el.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme().selected))
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::PrDraft.t())),
            )
        });
    div()
        .id("pr-mode-description")
        .w_full()
        .child(
            div()
                // Fill the pane (edge-aligned with the commit strip above); a
                // measure cap left the card visibly narrower than the strip.
                .w_full()
                .rounded_lg()
                .bg(rgb(card_bg()))
                .border_1()
                .border_color(card_border())
                .px_6()
                .py_5()
                .text_color(rgb(theme().text_main))
                .child(card_header)
                .child(
                    TextView::markdown(
                        ("pr-mode-description-md", pr.number as usize),
                        SharedString::from(kagi_ui_core::markdown::flatten_html_blocks(&body)),
                    )
                    .plugin(kagi_ui_core::markdown::MarkdownImages::remote())
                    // Drag to select, ⌘C to copy — gpui-component's Root
                    // collects the window selection across every selectable
                    // TextView (user request: all text, code included).
                    .selectable(true)
                    .style(style),
                ),
        )
        .into_any_element()
}

/// "Loading…" with the bobbing dots, while a tab's own fetch is still out
/// (Review: reviews + comments; Overview: merge status). Callers keep it
/// outside the scroll panes: `with_animation` doesn't tick inside one.
pub(super) fn render_loading() -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_3()
        .py_3()
        .bg(rgb(card_pane_bg()))
        .child(super::render_body::render_loading_dots())
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(Msg::EditorWorkspaceLoading.t())),
        )
}

/// One entry of the conversation: a review, an issue comment or a line
/// comment, flattened so the feed's virtualized list can render any one of
/// them without building the others (a PR with dozens of Copilot comments
/// stuttered when every card was laid out every frame - user report).
#[derive(Clone)]
pub struct Entry {
    author: String,
    verdict: Option<(String, u32)>,
    body: String,
    at: String,
    /// `path:line` for a line-level comment (Copilot / Codex suggestions).
    anchor: Option<String>,
    /// The comment carries a ```suggestion block.
    suggestion: bool,
    /// Severity tag lifted out of the body (Codex `P1`, Copilot `MUST`).
    tag: Option<kagi_domain::github::CommentTag>,
    /// The diff hunk GitHub shows above a line comment - the code the
    /// comment is about. This is what reads as "the code proposal" on
    /// github.com (Copilot's comments carry it instead of a
    /// ```suggestion fence).
    hunk: String,
}

/// The conversation, oldest first: submitted reviews, issue comments and line
/// comments in one timeline (ISO-8601 sorts lexically). Pure; rebuilt when the
/// tab's conversation lands, not per frame.
pub(super) fn conversation_entries(
    reviews: &[Review],
    comments: &[Comment],
    line_comments: &[ReviewComment],
) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for r in reviews {
        let (tag, r_body) = kagi_domain::github::extract_comment_tag(&r.body);
        let verdict = match r.state.as_str() {
            "APPROVED" => Some((Msg::PrReviewApproved.t().to_string(), theme().color_success)),
            "CHANGES_REQUESTED" => {
                Some((Msg::PrReviewChanges.t().to_string(), theme().color_warning))
            }
            _ => None,
        };
        entries.push(Entry {
            author: r.author.clone(),
            verdict,
            body: r_body,
            at: r.submitted_at.clone(),
            anchor: None,
            suggestion: false,
            tag,
            hunk: String::new(),
        });
    }
    for c in comments {
        entries.push(Entry {
            author: c.author.clone(),
            verdict: None,
            body: c.body.clone(),
            at: c.created_at.clone(),
            anchor: None,
            suggestion: false,
            tag: None,
            hunk: String::new(),
        });
    }
    // Line comments — where Copilot / Codex put code suggestions. They carry
    // the file:line they are anchored to; the ```suggestion fence in the body
    // renders as a code block through the same markdown path.
    for c in line_comments {
        // Codex ships its priority as a shields.io image badge and Copilot as
        // a `[MUST]` prefix; both become a native chip and leave the body.
        let (tag, body) = kagi_domain::github::extract_comment_tag(&c.body);
        entries.push(Entry {
            author: c.author.clone(),
            verdict: None,
            body,
            at: c.created_at.clone(),
            anchor: Some(if c.line > 0 {
                format!("{}:{}", c.path, c.line)
            } else {
                c.path.clone()
            }),
            suggestion: c.has_suggestion(),
            tag,
            hunk: c.diff_hunk.clone(),
        });
    }
    entries.sort_by(|a, b| a.at.cmp(&b.at));
    entries
}

/// One conversation card. Bodies go through the same markdown pipeline (and
/// the same sanitiser) as the description.
fn render_entry(
    number: u64,
    i: usize,
    e: &Entry,
    avatars: &kagi_ui_core::avatar::AvatarImages,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use gpui_component::text::{TextView, TextViewStyle};
    use gpui_component::ActiveTheme as _;

    let style = TextViewStyle {
        heading_base_font_size: theme::scaled_px(15.),
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        ..Default::default()
    };
    let body = kagi_domain::message::sanitize_markdown_for_view(&e.body);
    let body = kagi_ui_editor::markdown::pad_inline_code(&body);
    div()
        .w_full()
        .rounded_lg()
        .bg(rgb(card_bg()))
        .border_1()
        .border_color(card_border())
        .px_4()
        .py_3()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .pb_2()
                .mb_2()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .child(kagi_ui_core::commit_header::avatar_circle(
                    18., &e.author, &e.author, avatars,
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&e.author)),
                )
                .children(e.tag.as_ref().map(|t| {
                    use kagi_domain::github::TagSeverity;
                    let c = match t.severity {
                        TagSeverity::High => theme().color_blocker,
                        TagSeverity::Medium => theme().color_warning,
                        TagSeverity::Low => theme().text_sub,
                    };
                    let (bg, border, fg) = super::theme::badge_style(c);
                    div()
                        .px_1()
                        .rounded_sm()
                        .bg(gpui::rgba(bg))
                        .border_1()
                        .border_color(gpui::rgba(border))
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(fg))
                        .child(SharedString::from(t.label.clone()))
                }))
                .children(e.verdict.as_ref().map(|(t, c)| {
                    div()
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(*c))
                        .text_xs()
                        .text_color(rgb(*c))
                        .child(SharedString::from(t.clone()))
                }))
                .when(e.suggestion, |el| {
                    el.child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(theme().color_branch))
                            .text_xs()
                            .text_color(rgb(theme().color_branch))
                            .child(SharedString::from(Msg::PrSuggestion.t())),
                    )
                })
                .child(div().flex_1())
                .children(e.anchor.as_ref().map(|a| {
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(super::MONO_FONT)
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(a.clone()))
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(e.at.clone())),
                ),
        )
        .when(!e.hunk.trim().is_empty(), |el| {
            el.child(render_diff_hunk(&e.hunk, number as usize * 1000 + i, cx))
        })
        .when(!body.trim().is_empty(), |el| {
            el.child(
                TextView::markdown(
                    ("pr-convo-md", number as usize * 1000 + i),
                    SharedString::from(kagi_ui_core::markdown::flatten_html_blocks(&body)),
                )
                .plugin(kagi_ui_core::markdown::MarkdownImages::remote())
                .selectable(true)
                .style(style.clone()),
            )
        })
        .into_any_element()
}

/// What sits at each index of the PR page's virtualized list (ADR-0200).
///
/// One list, one scroll, rendered item by item by `gpui::list`: only what is
/// on screen is laid out, so a PR with a long conversation scrolls as smoothly
/// as one with none (user report). The 概要 / レビュー tabs reveal item 0 and
/// the conversation heading respectively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeedItem {
    Headline,
    Properties,
    Checks,
    MergeCard,
    Description,
    /// The conversation heading; the anchor the レビュー tab reveals.
    Conversation,
    /// "None yet" / "loading" - only when there are no entries.
    Empty,
    Entry(usize),
}

/// The page's items, in reading order, from what the tab currently holds.
/// Optional blocks are simply absent, so indices are recomputed per frame and
/// the heading's index is looked up rather than assumed.
pub(super) fn feed_items(pr: &PullRequest, merge_card: bool, entries: usize) -> Vec<FeedItem> {
    let mut items = vec![FeedItem::Headline, FeedItem::Properties];
    if !pr.checks.is_empty() {
        items.push(FeedItem::Checks);
    }
    if merge_card {
        items.push(FeedItem::MergeCard);
    }
    items.push(FeedItem::Description);
    items.push(FeedItem::Conversation);
    if entries == 0 {
        items.push(FeedItem::Empty);
    }
    items.extend((0..entries).map(FeedItem::Entry));
    items
}

/// Render one item of the page for the active tab `tab_ix`. Called by the
/// list for the visible items only, through `cx.processor`, so it has the
/// same `&mut KagiApp` the rest of the PR page renders with.
pub(super) fn render_feed_item(
    app: &mut KagiApp,
    tab_ix: usize,
    ix: usize,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Decide what the item is by borrowing; clone only what that item needs.
    // Every visible entry used to copy the whole `PullRequest` (body, checks,
    // labels) and the merge status - the cost the `Rc` on the entries was
    // there to avoid (review finding, `w5:p19`).
    let (item, number, review_state) = {
        let Some(tab) = app.pr_mode().and_then(|m| m.tabs.get(tab_ix)) else {
            return div().into_any_element();
        };
        let has_merge_card = tab
            .merge_status
            .as_ref()
            .map(|status| {
                super::pr_merge_status::render(
                    &super::pr_merge_status::view_from(status, tab.pr.review),
                    cx,
                )
                .is_some()
            })
            .unwrap_or(false);
        let items = feed_items(&tab.pr, has_merge_card, tab.feed_entries.len());
        let Some(item) = items.get(ix).copied() else {
            return div().into_any_element();
        };
        (item, tab.pr.number, tab.pr.review)
    };
    let block = |el: gpui::AnyElement| div().w_full().pb_3().child(el).into_any_element();
    // The page-level cards are one item each and need the PR; cloning it once
    // for those few items is fine. Entries do not go through this path.
    let pr_for_card = |app: &KagiApp| {
        app.pr_mode()
            .and_then(|m| m.tabs.get(tab_ix))
            .map(|t| t.pr.clone())
    };
    match item {
        FeedItem::Headline => match pr_for_card(app) {
            Some(pr) => block(super::e2e::measure_control(
                "pr-mode-headline",
                super::pr_page::render_pr_headline(app, &pr),
            )),
            None => div().into_any_element(),
        },
        FeedItem::Properties => match pr_for_card(app) {
            Some(pr) => block(super::e2e::measure_control(
                "pr-mode-properties",
                super::pr_page::render_pr_properties(app, &pr, cx),
            )),
            None => div().into_any_element(),
        },
        FeedItem::Checks => match pr_for_card(app) {
            Some(pr) => super::pr_page::render_checks_card(app, &pr, cx)
                .map(block)
                .unwrap_or_else(|| div().into_any_element()),
            None => div().into_any_element(),
        },
        FeedItem::MergeCard => {
            let status = app
                .pr_mode()
                .and_then(|m| m.tabs.get(tab_ix))
                .and_then(|t| t.merge_status.clone());
            status
                .as_ref()
                .and_then(|status| {
                    super::pr_merge_status::render(
                        &super::pr_merge_status::view_from(status, review_state),
                        cx,
                    )
                })
                .map(block)
                .unwrap_or_else(|| div().into_any_element())
        }
        FeedItem::Description => match pr_for_card(app) {
            Some(pr) => block(description_card(&pr, cx)),
            None => div().into_any_element(),
        },
        FeedItem::Conversation => {
            let count = app
                .pr_mode()
                .and_then(|m| m.tabs.get(tab_ix))
                .map(|t| t.feed_entries.len())
                .unwrap_or(0);
            block(super::e2e::measure_control(
                "pr-feed-review",
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(format!(
                        "{} ({})",
                        Msg::PrModeReview.t(),
                        count
                    ))),
            ))
        }
        // "None" and "not fetched yet" are different things to say; the
        // animated dots live above the feed, outside this scroll pane.
        FeedItem::Empty => {
            let loaded = app
                .pr_mode()
                .and_then(|m| m.tabs.get(tab_ix))
                .map(|t| t.conversation_loaded)
                .unwrap_or(false);
            block(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(if loaded {
                        Msg::PrModeNoReview.t()
                    } else {
                        Msg::EditorWorkspaceLoading.t()
                    }))
                    .into_any_element(),
            )
        }
        FeedItem::Entry(i) => {
            // One `Rc` bump for the entries, one `Arc`-map clone for the
            // avatars; the entry itself is borrowed out of the `Rc`.
            let entries = app
                .pr_mode()
                .and_then(|m| m.tabs.get(tab_ix))
                .map(|t| t.feed_entries.clone());
            match entries.as_ref().and_then(|e| e.get(i)) {
                Some(entry) => {
                    let avatars = app.avatars.images.clone();
                    block(super::e2e::measure_control(
                        format!("pr-feed-entry-{i}"),
                        render_entry(number, i, entry, &avatars, cx),
                    ))
                }
                None => div().into_any_element(),
            }
        }
    }
}

/// The PR's page as one virtualized list (see [`FeedItem`]). `tab_ix` is the
/// active tab; the list's own `ListState` lives on that tab so its scroll
/// survives a tab switch. A tab press has already asked for its anchor
/// through `PrModeState::feed_anchor`; it is consumed here, once.
pub(super) fn render_feed(
    app: &mut KagiApp,
    tab_ix: usize,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let (state, count, heading_ix) = {
        let Some(tab) = app.pr_mode().and_then(|m| m.tabs.get(tab_ix)) else {
            return div().into_any_element();
        };
        let has_merge_card = tab
            .merge_status
            .as_ref()
            .map(|status| {
                super::pr_merge_status::render(
                    &super::pr_merge_status::view_from(status, tab.pr.review),
                    cx,
                )
                .is_some()
            })
            .unwrap_or(false);
        let items = feed_items(&tab.pr, has_merge_card, tab.feed_entries.len());
        let heading_ix = items
            .iter()
            .position(|i| *i == FeedItem::Conversation)
            .unwrap_or(0);
        (tab.feed_list.clone(), items.len(), heading_ix)
    };
    // The list only learns its length through `reset`, which also drops the
    // scroll position - so only when the length actually changed (a fetch
    // landing, a card appearing), never per frame.
    if state.item_count() != count {
        state.reset(count);
    }
    if let Some(anchor) = app.pr_mode_mut().and_then(|m| m.feed_anchor.take()) {
        // `scroll_to`, not `scroll_to_reveal_item`: reveal is the *minimal*
        // scroll, which brought the conversation heading in at the bottom
        // edge with every review still below the fold - "the reviews are not
        // shown" (user report). A tab press puts its section at the top.
        state.scroll_to(gpui::ListOffset {
            item_ix: if anchor == 0 { 0 } else { heading_ix },
            offset_in_item: px(0.),
        });
    }
    let render = cx.processor(move |app: &mut KagiApp, ix: usize, _window, cx| {
        render_feed_item(app, tab_ix, ix, cx)
    });
    // The same shell `render_diff_list` gives its list: a relative flex column
    // that hands the list `flex_1`, plus the theme's scrollbar. Anything
    // looser (a bare wrapper, or `size_full` on the list) resolved the list
    // to 0px high and it drew nothing.
    let scrollbar_handle = state.clone();
    div()
        .id("pr-mode-feed")
        .flex_1()
        .min_h(px(0.))
        // `h_full` + `overflow_hidden`, as the diff pane: a `list` takes the
        // height it is *given*, and a flex child with only `flex_1` gives it
        // none until the column's height is definite.
        .h_full()
        .overflow_hidden()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(card_pane_bg()))
        .px_4()
        .pt_4()
        .child(super::render_helpers::with_vertical_scrollbar(
            "pr-mode-feed-scroll",
            &scrollbar_handle,
            gpui::list(state, move |ix, window, cx| render(ix, window, cx))
                .flex_1()
                .min_h(px(0.)),
            true,
        ))
        .into_any_element()
}

/// The `diff_hunk` GitHub attaches to a line comment — the code the comment
/// is about, and what reads as "the code proposal" on github.com (Copilot and
/// Codex comments carry it instead of a ```suggestion fence).
///
/// Drawn with kagi's own diff colours rather than as a ```diff fenced block:
/// routing it through the markdown renderer made tree-sitter colour it with a
/// generic diff palette that clashed with the diff panes next door (user
/// report). A copy button carries the copyability that the fenced version got
/// for free.
///
/// Only the last few lines are shown: the hunk can be 30+ lines of context and
/// the comment is about its end.
fn render_diff_hunk(hunk: &str, id: usize, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    const MAX_LINES: usize = 12;
    let all: Vec<&str> = hunk.lines().collect();
    let skipped = all.len().saturating_sub(MAX_LINES);
    let shown = &all[skipped..];

    let full = hunk.to_string();
    let copy = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        cx.stop_propagation();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(full.clone()));
        this.push_toast(
            ToastKind::Info,
            SharedString::from(Msg::PrHunkCopied.t()),
            cx,
        );
    });

    let mut rows = div().flex().flex_col().py_1();
    if skipped > 0 {
        rows = rows.child(
            div()
                .px_2()
                .text_xs()
                .font_family(super::MONO_FONT)
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!(
                    "\u{2026} {} more lines",
                    skipped
                ))),
        );
    }
    for line in shown {
        let (bg, fg) = match line.as_bytes().first() {
            Some(b'+') => (Some(theme().diff_added_bg), theme().change_added),
            Some(b'-') => (Some(theme().diff_removed_bg), theme().change_deleted),
            Some(b'@') => (None, theme().diff_hunk),
            _ => (None, theme().text_sub),
        };
        rows = rows.child(
            div()
                .w_full()
                .px_2()
                .whitespace_nowrap()
                .overflow_hidden()
                .font_family(super::MONO_FONT)
                .text_xs()
                .text_color(rgb(fg))
                .when_some(bg, |el, b| el.bg(rgb(b)))
                .child(SharedString::from(line.to_string())),
        );
    }

    div()
        .id(("pr-convo-hunk", id))
        .relative()
        .w_full()
        .mb_2()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().bg_base))
        .child(rows)
        // Copy the WHOLE hunk (not just the shown tail) — hover-revealed so
        // it never competes with the code.
        .child(
            div()
                .absolute()
                .top_1()
                .right_1()
                .id(("pr-convo-hunk-copy", id))
                .p_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .border_1()
                .border_color(rgb(theme().selected))
                .cursor_pointer()
                // Quiet until hovered (no `visible_on_hover` in this gpui).
                .opacity(0.55)
                .hover(|s| s.bg(rgb(theme().selected)).opacity(1.0))
                .tooltip(|w, cx| {
                    gpui_component::tooltip::Tooltip::new(Msg::PrHunkCopy.t()).build(w, cx)
                })
                // Swallow the mouse-DOWN too, not just the click: otherwise it
                // reaches the selectable TextViews underneath and starts a
                // window text-selection that then tracks the cursor after the
                // button is released (user report).
                .on_mouse_down(gpui::MouseButton::Left, |_e, _w, cx| {
                    cx.stop_propagation();
                })
                .on_click(copy)
                .child(
                    gpui::svg()
                        .path("icons/copy.svg")
                        .w(theme::scaled_px(12.))
                        .h(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_sub)),
                ),
        )
        .into_any_element()
}
