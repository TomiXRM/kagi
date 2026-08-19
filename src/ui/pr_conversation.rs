//! PR conversation rendering: the description card, the review/comment
//! timeline, and the inline `diff_hunk` suggestion blocks.
//!
//! Split out of `pr_mode.rs` — these are pure renderers over a `PrTab`, with
//! no state of their own.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{Comment, PullRequest, Review, ReviewComment};

use super::i18n::Msg;
use super::pr_mode::{card_bg, card_border, card_pane_bg, ci_glyph};
use super::theme::{self, theme};
use super::types::ToastKind;
use super::KagiApp;

/// The PR description as rendered markdown, in the diff area.
pub(super) fn render_description(pr: &PullRequest, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
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
                .child(SharedString::from(format!("@{}", pr.author))),
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
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_y_scroll()
        .bg(rgb(card_pane_bg()))
        .p_4()
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
                        SharedString::from(body),
                    )
                    // Drag to select, ⌘C to copy — gpui-component's Root
                    // collects the window selection across every selectable
                    // TextView (user request: all text, code included).
                    .selectable(true)
                    .style(style),
                ),
        )
        .into_any_element()
}

/// The review conversation: submitted reviews and issue comments, newest
/// last, each as a card with the author's verdict. Bodies go through the same
/// markdown pipeline (and the same sanitiser) as the description.
pub(super) fn render_conversation(
    pr: &PullRequest,
    reviews: &[Review],
    comments: &[Comment],
    line_comments: &[ReviewComment],
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

    // One timeline, ordered by timestamp (ISO-8601 sorts lexically).
    struct Entry {
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
        /// The diff hunk GitHub shows above a line comment — the code the
        /// comment is about. This is what reads as "the code proposal" on
        /// github.com (Copilot's comments carry it instead of a
        /// ```suggestion fence).
        hunk: String,
    }
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

    let mut col = div()
        .id("pr-mode-conversation")
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_y_scroll()
        .bg(rgb(card_pane_bg()))
        .p_4()
        .flex()
        .flex_col()
        .gap_3();

    if entries.is_empty() {
        return col
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::PrModeNoReview.t())),
            )
            .into_any_element();
    }

    for (i, e) in entries.iter().enumerate() {
        let body = kagi_domain::message::sanitize_markdown_for_view(&e.body);
        let body = kagi_ui_editor::markdown::pad_inline_code(&body);
        col = col.child(
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
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(format!("@{}", e.author))),
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
                    el.child(render_diff_hunk(&e.hunk, pr.number as usize * 1000 + i, cx))
                })
                .when(!body.trim().is_empty(), |el| {
                    el.child(
                        TextView::markdown(
                            ("pr-convo-md", pr.number as usize * 1000 + i),
                            SharedString::from(body),
                        )
                        .selectable(true)
                        .style(style.clone()),
                    )
                }),
        );
    }
    col.into_any_element()
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
