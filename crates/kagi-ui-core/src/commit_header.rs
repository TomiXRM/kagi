//! Shared commit subject/meta/body block — used by the Editor Workspace's
//! History pane header (`kagi-ui-editor`) and File History's detail pane
//! (`kagi-ui-file-history`); both already work with `CommitSummary`.
//!
//! Pure content, deliberately with no scroll/sizing baked in: Editor wraps
//! the result in its own fixed-ratio flex box (a header pinned above a
//! separately-scrolling commit list), while File History drops it straight
//! into its own single scrolling column alongside other rows. Neither
//! layout belongs here — only the two crates' shared *content* does.
//!
//! Graph mode's Inspector (`src/ui/inspector.rs`) shows similar information
//! but is bin-side, keyed off a different type (`CommitDetail`, richer and
//! deeply entangled with the files list / actions row / drag divider) — not
//! folded into this shared component; out of scope for this cheap a win.

use gpui::prelude::*;
use gpui::{div, rgb, AnyElement, SharedString};

use kagi_domain::file_history::CommitSummary;
use kagi_domain::message::reflow_message;
use kagi_domain::trailers::parse_coauthors;

use crate::avatar::{avatar_color, avatar_initial, AvatarImages};
use crate::i18n::Msg;
use crate::theme::{self, theme};

/// One round avatar: the resolved GitHub/Gravatar image when the background
/// resolution pass (ADR-0037/0123, bin-side) has one for `email`, else the
/// deterministic initial-on-colour circle (T020 fallback). Same treatment
/// Graph mode's Inspector uses — `size` is 18px for a commit's own author,
/// 16px for a co-author row, matching `inspector.rs`.
fn avatar_circle(
    size: f32,
    email: &str,
    display_name: &str,
    avatars: &AvatarImages,
) -> impl IntoElement {
    let circle = div()
        .w(theme::scaled_px(size))
        .h(theme::scaled_px(size))
        .flex_shrink_0()
        .rounded_full()
        .overflow_hidden();
    match avatars.get(email).cloned() {
        Some(image) => circle.child(
            gpui::img(gpui::ImageSource::Image(image))
                .size_full()
                .rounded_full(),
        ),
        None => circle
            .flex()
            .items_center()
            .justify_center()
            .bg(avatar_color(email))
            .text_xs()
            .text_color(rgb(theme().bg_base))
            .child(SharedString::from(avatar_initial(display_name))),
    }
}

/// Subject, a compact meta line (avatar · short hash · author · date), the
/// reflowed body (if any and non-blank), and any `Co-authored-by:` trailers
/// with their own avatars. `None` renders a "select a commit" placeholder
/// instead.
///
/// `avatars` is the host-owned resolved-image map, pushed into the embedding
/// entity each frame (the pane crates can't reach `KagiApp` themselves) — an
/// empty map is fine and simply yields initial circles throughout.
pub fn render_commit_header(commit: Option<&CommitSummary>, avatars: &AvatarImages) -> AnyElement {
    let Some(commit) = commit else {
        return div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(Msg::CommitHeaderSelectPrompt.t())
            .into_any_element();
    };

    // `author_date` is `git log`'s strict-ISO8601 (`%aI`) — the leading 10
    // bytes are always `YYYY-MM-DD`; no date parser needed for this compact
    // line.
    let date = commit.author_date.get(0..10).unwrap_or("");
    let meta = format!(
        "{} \u{b7} {} \u{b7} {}",
        commit.short_hash, commit.author_name, date
    );

    let mut block = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .whitespace_normal()
                .child(SharedString::from(commit.subject.clone())),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(avatar_circle(
                    18.,
                    &commit.author_email,
                    &commit.author_name,
                    avatars,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(gpui::px(0.))
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .truncate()
                        .child(SharedString::from(meta)),
                ),
        );

    if let Some(body) = commit.body.as_ref().filter(|b| !b.trim().is_empty()) {
        block = block.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .whitespace_normal()
                .child(SharedString::from(reflow_message(body))),
        );

        // `Co-authored-by:` trailers (W18-COAUTHOR-COPY), each with its own
        // avatar — same 16px circle Inspector gives them.
        let coauthors = parse_coauthors(body);
        if !coauthors.is_empty() {
            let mut list = div().flex().flex_col().gap_1().child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(Msg::CoAuthoredBy.t()),
            );
            for ca in &coauthors {
                // A trailer may carry only a name or only an email; show
                // whichever identifies the person, and key the avatar off the
                // email (empty email → a stable "no email" colour, same as
                // Inspector).
                let display = if ca.name.is_empty() {
                    ca.email.clone()
                } else if ca.email.is_empty() {
                    ca.name.clone()
                } else {
                    format!("{} <{}>", ca.name, ca.email)
                };
                let initial_from = if ca.name.is_empty() {
                    &ca.email
                } else {
                    &ca.name
                };
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(avatar_circle(16., &ca.email, initial_from, avatars))
                        .child(
                            div()
                                .flex_1()
                                .min_w(gpui::px(0.))
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .truncate()
                                .child(SharedString::from(display)),
                        ),
                );
            }
            block = block.child(list);
        }
    }
    block.into_any_element()
}
