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

use crate::i18n::Msg;
use crate::theme::theme;

/// Subject, a compact meta line (short hash · author · date), and the
/// reflowed body (if any and non-blank). `None` renders a "select a commit"
/// placeholder instead.
pub fn render_commit_header(commit: Option<&CommitSummary>) -> AnyElement {
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
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(meta)),
        );

    if let Some(body) = commit.body.as_ref().filter(|b| !b.trim().is_empty()) {
        block = block.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .whitespace_normal()
                .child(SharedString::from(reflow_message(body))),
        );

        // `Co-authored-by:` trailers (W18-COAUTHOR-COPY) — text only, no
        // avatar circle: Inspector's avatar needs the GitHub/Gravatar fetch
        // + `KagiApp`-owned image cache (ADR-0037/0123), bin-side I/O this
        // pure component deliberately doesn't take on.
        let coauthors = parse_coauthors(body);
        if !coauthors.is_empty() {
            let mut list = div().flex().flex_col().gap_px().child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(Msg::CoAuthoredBy.t()),
            );
            for ca in &coauthors {
                let display = if ca.name.is_empty() {
                    ca.email.clone()
                } else if ca.email.is_empty() {
                    ca.name.clone()
                } else {
                    format!("{} <{}>", ca.name, ca.email)
                };
                list = list.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .truncate()
                        .child(SharedString::from(display)),
                );
            }
            block = block.child(list);
        }
    }
    block.into_any_element()
}
