//! The timeline chrome Issues and PRs share (ADR-0201, #750).
//!
//! One copy of what a row is made of — the 40px avatar column, the
//! `@login · age` meta line, the markdown body, and the composer's frame and
//! controls — so the Issues feed, the PR feed and the PR home table cannot
//! drift apart by a padding value.
//!
//! Renderers only: no state, no I/O. What a row *says* stays with the caller.

use gpui::{div, prelude::*, px, rgb, AnyElement, Div, ElementId, SharedString, Stateful};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use kagi_ui_core::avatar::AvatarImages;

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};

/// Avatar diameter — the row's left column, not a decoration in the text.
pub(super) const AVATAR: f32 = 40.;
/// The page's left/right gutter, the same on rows and on the composer.
pub(super) const GUTTER: f32 = 24.;
/// Avatar-to-content gap.
pub(super) const GAP: f32 = 14.;
/// Row padding above and below the content.
pub(super) const ROW_PY: f32 = 16.;

/// The row frame: avatar column, content, and the 1px line that is the only
/// separator — borderless rows, no cards (ADR-0201).
///
/// The content is the caller's: a column of meta/body for a feed post, a
/// strip of fixed-width cells for the PR home table.
pub(super) fn row(
    id: impl Into<ElementId>,
    author: &str,
    avatars: &AvatarImages,
    content: Div,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .min_w(px(0.))
        .flex()
        .flex_row()
        .gap(theme::scaled_px(GAP))
        .px(theme::scaled_px(GUTTER))
        .py(theme::scaled_px(ROW_PY))
        .border_b_1()
        .border_color(rgb(theme().selected))
        .text_color(rgb(theme().text_main))
        .child(kagi_ui_core::commit_header::avatar_circle_with_initials(
            AVATAR, author, author, avatars,
        ))
        .child(content)
}

/// A row that opens what it shows: pointer and hover fill, one definition for
/// the Issues list and the PR home table.
pub(super) fn clickable(row: Stateful<Div>) -> Stateful<Div> {
    row.cursor_pointer().hover(|s| s.bg(rgb(theme().surface)))
}

/// The content column of a row or composer: everything right of the avatar.
/// The `min_w(0)` is what lets a long line truncate instead of widening the
/// pane.
pub(super) fn content_column() -> Div {
    div().flex_1().min_w(px(0.)).flex().flex_col()
}

/// `@login` and one muted trailing fact — an age, or `#N · age`.
///
/// Returns the row so a caller can hang its own chips off it (a review's
/// verdict, a line comment's `path:line`).
pub(super) fn meta(author: &str, trailing: &str) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap_2()
        .text_xs()
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(theme().text_main))
                .child(safe_text(&format!("@{author}"))),
        )
        .child(
            div()
                .text_color(rgb(theme().text_muted))
                .child(safe_text(trailing)),
        )
}

/// How long ago an RFC-3339 stamp was, falling back to the stamp itself when
/// it cannot be parsed — "2026-09-01T00:00:00Z" still says more than nothing.
pub(super) fn age(created_at: &str) -> String {
    kagi_ui_core::time_parse::iso_to_epoch(created_at)
        .map(|at| kagi_ui_core::time::relative_time(at, kagi_ui_core::time::now_unix_secs()))
        .unwrap_or_else(|| created_at.to_string())
}

/// The markdown style a body is drawn with: the caller's heading scale, this
/// window's syntax theme.
pub(super) fn markdown_style(heading_base: f32, cx: &gpui::App) -> TextViewStyle {
    TextViewStyle {
        heading_base_font_size: theme::scaled_px(heading_base),
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        ..Default::default()
    }
}

/// A body through the GitHub-markdown pipeline the feeds share
/// (`kagi_ui_editor::markdown::prepare_github_markdown`): normalise the text,
/// turn images into links so a remote-origin comment cannot make the app
/// fetch anything, pad inline code, flatten HTML blocks — then render with
/// window selection, in the typography literal code needs
/// (`literal_text_features`: no contextual alternates, so `<!--` stays four
/// characters).
pub(super) fn body_markdown(
    id: impl Into<ElementId>,
    body: &str,
    style: TextViewStyle,
) -> AnyElement {
    TextView::markdown(
        id,
        SharedString::from(kagi_ui_editor::markdown::prepare_github_markdown(body)),
    )
    .selectable(true)
    .style(style)
    .font_features(kagi_ui_editor::markdown::literal_text_features())
    .into_any_element()
}

/// The composer's text box: borderless, at the feed's reading size. The
/// entity is the caller's — this only styles the box it is drawn in, so a
/// single-line/multi-line or auto-grow choice made at creation still stands.
pub(super) fn body_input(input: &gpui::Entity<InputState>) -> Input {
    Input::new(input)
        .appearance(false)
        .bordered(false)
        .focus_bordered(false)
        .text_size(theme::scaled_px(15.))
        .line_height(theme::scaled_px(25.5))
}

/// The state dot and its label — open/closed, and nothing that needs a badge.
pub(super) fn state_dot(label: impl Into<SharedString>, color: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .child(
            div()
                .w(theme::scaled_px(8.))
                .h(theme::scaled_px(8.))
                .rounded_full()
                .bg(rgb(color)),
        )
        .child(label.into())
}

/// The composer's frame: the viewer's own avatar in the row's avatar column,
/// so the box you write in lines up with the posts you are reading.
///
/// Slightly taller above than below — the box needs air under the row it
/// follows, not under its own buttons.
///
/// The divider is the caller's: an Issues composer sits at the head of the
/// feed and carries the line below it, a PR composer is pinned at the foot
/// and carries it above.
pub(super) fn composer_frame(
    id: impl Into<ElementId>,
    viewer: &str,
    avatars: &AvatarImages,
    content: Div,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .flex()
        .flex_row()
        .gap(theme::scaled_px(GAP))
        .px(theme::scaled_px(GUTTER))
        .pt(theme::scaled_px(20.))
        .pb(theme::scaled_px(14.))
        .child(kagi_ui_core::commit_header::avatar_circle_with_initials(
            AVATAR, viewer, viewer, avatars,
        ))
        .child(content)
}

/// Edit/preview as **one** control: the eye offers the preview, the pen offers
/// the way back. Two labelled buttons spent a whole row saying which mode you
/// were already in.
pub(super) fn mode_toggle(
    id: impl Into<SharedString>,
    preview: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Button {
    let (icon, tooltip) = if preview {
        ("icons/square-pen.svg", Msg::ComposerWrite.t())
    } else {
        ("icons/eye.svg", Msg::ComposerPreview.t())
    };
    Button::new(id.into())
        .icon(Icon::empty().path(icon))
        .small()
        .tooltip(tooltip)
        .on_click(on_click)
}

/// The submit. Amber only while it can actually be pressed: an unavailable
/// action tinted like an available one is a lie the reader acts on, so the
/// disabled state stays on Button's neutral path.
pub(super) fn submit(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Button {
    let button = Button::new(id.into())
        .icon(Icon::empty().path("icons/comment-send.svg"))
        .label(label.into());
    if disabled { button } else { button.warning() }
        .rounded(px(999.))
        .h(theme::scaled_px(36.))
        .px_3()
        .disabled(disabled)
        .on_click(on_click)
}

/// The composer's one-line status: where the text you typed currently lives.
/// Muted, under the controls — it reports, it does not ask for anything.
pub(super) fn draft_status(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .child(text.into())
}
