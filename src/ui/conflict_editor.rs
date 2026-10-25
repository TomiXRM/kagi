//! Shared conflict code rows, syntax, selection bands and Result editor.
//! Presentation pages live in `pages`; all choices still use ResolutionBuffer.
//! Source panes show the whole file with a distinct selected-hunk highlight.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    canvas, div, prelude::*, px, relative, rgb, uniform_list, AnyElement, Context, SharedString,
    UniformListScrollHandle, Window,
};
use gpui_component::button::Button;
use gpui_component::input::Input;
use gpui_component::scroll::Scrollbar;
use gpui_component::{Disableable as _, Sizable as _};

use kagi_git::resolution::{LineOrigin, Region, SelectionSide, TriState};

use super::button_style::{ButtonRole, KagiButton};
use super::conflict_view::ConflictMode;
use super::conflict_view::ConflictPage;
use super::conflict_view::ConflictView;
use super::conflict_view::EditorChrome;
use super::i18n::Msg;
use super::terminal;
use super::theme::{self, theme};
mod pages;
use super::e2e;
mod result;
mod rows;
mod syntax;
#[cfg(test)]
mod tests;
use pages::{render_panes, tool_button};
pub(super) use result::render_saved_preview;
pub use result::ResultHlCache;
use result::*;
use rows::*;
pub use syntax::SideHlCache;
use syntax::*;

/// Render the selected hunk or full Result review, without the file overview.
pub fn render_editor(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    // Pages share the existing row and Result renderers, never a second engine.
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .bg(rgb(theme().bg_base))
        .child(render_panes(mode, chrome, path, cx))
        .into_any_element()
}

fn guidance_pane(msg: &str) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .child(
            div()
                .max_w(theme::scaled_px(420.))
                .text_size(theme::scaled_px(13.))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(msg.to_string())),
        )
        .into_any_element()
}

/// Source viewport; the shared column headers precede the notification anchor.
fn pane(id: &'static str, editor: impl IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .min_h(px(0.))
                .child(editor),
        )
}

pub(super) fn source_headers(mode: &ConflictMode) -> Option<gpui::AnyElement> {
    let file = mode
        .selected_file
        .and_then(|index| mode.session.files.get(index))?;
    mode.buffer.hunk_model(&file.path)?;
    let labels = mode.labels();
    Some(
        div()
            .relative()
            .flex()
            .flex_shrink_0()
            .child(e2e::measure_inside("conflict-source-headers"))
            .child(pane_header(
                format!("{} — {}", labels.current.name, Msg::EditorCurrentSide.t()),
                theme().color_branch,
            ))
            .child(
                div()
                    .w(theme::scaled_px(12.))
                    .flex_shrink_0()
                    .bg(rgb(theme().panel)),
            )
            .child(pane_header(
                format!("{} — {}", labels.incoming.name, Msg::EditorIncomingSide.t()),
                theme().color_remote,
            ))
            .into_any_element(),
    )
}

fn pane_header(label: String, accent: u32) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .min_w(px(0.))
        .items_center()
        .gap(theme::scaled_px(8.))
        .h(theme::scaled_px(26.))
        .flex_shrink_0()
        .px(theme::scaled_px(12.))
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .size(theme::scaled_px(8.))
                .rounded_full()
                .bg(rgb(accent))
                .flex_shrink_0(),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(theme::scaled_px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(theme().text_main))
                .child(label),
        )
}

/// Save alone freezes the existing revision-bound request and defers to its owner.
pub(super) fn save_button(
    path: &std::path::Path,
    writer_busy: bool,
    external_change: bool,
    refused: bool,
    ready: bool,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let p_save = path.to_path_buf();
    let role = if ready || refused {
        ButtonRole::Primary
    } else {
        ButtonRole::Secondary
    };
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record_button_role("conflict-save", role);
    let save = cx.listener(
        move |view: &mut ConflictView, _: &gpui::ClickEvent, window, cx| {
            view.conflict_show_page(ConflictPage::Review, cx);
            let weak_app = view.app.clone();
            let intent = view.freeze_save_intent(&p_save);
            cx.spawn_in(window, async move |_, acx| {
                let _ =
                    weak_app.update_in(acx, |app, _, cx| app.accept_conflict_intent(intent, cx));
            })
            .detach();
        },
    );
    super::e2e::measure_control(
        if refused {
            "conflict-save-again"
        } else {
            "conflict-save"
        },
        tool_button(
            if refused {
                "conflict-save-again"
            } else {
                "editor-save"
            },
            if refused {
                Msg::ConflictSaveAgain.t()
            } else {
                Msg::EditorSave.t()
            },
            role,
            save,
            cx,
        )
        .disabled(writer_busy || external_change || (!ready && !refused))
        .aria_description(if external_change {
            Msg::ConflictWorktreeChangedHelp.t()
        } else if writer_busy {
            Msg::OpInProgress.t()
        } else if !ready {
            Msg::ConflictBlockerUnresolved.t()
        } else {
            ""
        }),
    )
    .into_any_element()
}
