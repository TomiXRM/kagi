//! Smart-commit / auto-update modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). Pure physical move — behaviour
//! unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_modal_title_row, ModalIcon};
use super::modals::EditorDirtyGuardModal;
use super::theme::{self, theme as current_theme};
use super::{smart_commit, KagiApp};
use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, IconName, Sizable as _};

// ──────────────────────────────────────────────────────────────
// Smart Commit modal renderer (T-COMMIT-016, ADR-0044)
// ──────────────────────────────────────────────────────────────

/// Render the Smart Commit consent / model-picker overlay.
///
/// * `Consent` — the first-time opt-in dialog carrying the four mandated
///   statements ([`smart_commit::CONSENT_LINES`]).  Confirm enables LLM
///   generation and proceeds to model selection.
/// * `ModelPicker` — choose one installed model; the choice is persisted.
pub(crate) fn render_smart_commit_modal(
    modal: smart_commit::SmartCommitModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let card = match modal {
        smart_commit::SmartCommitModal::Consent => {
            let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.cancel_smart_modal(cx);
            });
            let confirm = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.confirm_smart_consent(cx);
            });
            let mut lines_col = div().flex().flex_col().gap_1();
            for line in smart_commit::CONSENT_LINES {
                lines_col = lines_col.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().color_branch))
                                .child(SharedString::from("•")),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(line)),
                        ),
                );
            }
            div()
                .w(theme::scaled_px(460.))
                .bg(rgb(current_theme().modal))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(render_modal_title_row(
                    SharedString::from("Enable Local LLM generation?"),
                    Some((IconName::Settings.into(), current_theme().color_success)),
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_sub))
                        .child(SharedString::from(
                            "Pressing Generate sends your staged diff to a local Ollama \
                             model on this machine. Please review:",
                        )),
                )
                .child(lines_col)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("smart-consent-cancel")
                                .label("Cancel")
                                .ghost()
                                .small()
                                .on_click(cancel),
                        )
                        .child(
                            KagiButton::accent(
                                "smart-consent-confirm",
                                "Enable & continue",
                                current_theme().color_success,
                                cx,
                            )
                            .small()
                            .on_click(confirm),
                        ),
                )
        }
        smart_commit::SmartCommitModal::ModelPicker { models } => {
            let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.cancel_smart_modal(cx);
            });
            let mut list = div().flex().flex_col().gap_1();
            for (i, m) in models.iter().enumerate() {
                let model_name = m.clone();
                let pick = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    this.choose_smart_model(model_name.clone(), window, cx);
                });
                list = list.child(
                    div()
                        .id(("smart-model", i))
                        .px_3()
                        .py_1()
                        .rounded_sm()
                        .bg(rgb(current_theme().surface))
                        .text_sm()
                        .text_color(rgb(current_theme().text_main))
                        .on_click(pick)
                        .hover(|s| s.bg(rgb(current_theme().selected)).cursor_pointer())
                        .child(SharedString::from(m.clone())),
                );
            }
            div()
                .w(theme::scaled_px(420.))
                .bg(rgb(current_theme().modal))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(render_modal_title_row(
                    SharedString::from("Select a local model"),
                    Some((IconName::Settings.into(), current_theme().color_branch)),
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_sub))
                        .child(SharedString::from(
                            "Choose which installed Ollama model to use. \
                             Your choice is remembered.",
                        )),
                )
                .child(list)
                .child(
                    div().flex().flex_row().justify_end().child(
                        Button::new("smart-model-cancel")
                            .label("Cancel")
                            .ghost()
                            .small()
                            .on_click(cancel),
                    ),
                )
        }
    };

    modal_overlay(card)
}

/// Auto-update detail modal (ADR-0082, T-AUTOUPDATE-001).
///
/// Shows current → latest, the chosen asset, release notes, and — Phase 1 — a
/// "Update now" button that downloads + verifies + installs + relaunches. Phase-0
/// fallbacks ("Open release page", "Skip this version") are always present.
pub(crate) fn render_update_modal(
    plan: kagi_domain::update::UpdatePlan,
    installing: bool,
    status: Option<SharedString>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.update_modal_open = false;
        cx.notify();
    });
    let update_now = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.start_update_install(cx);
    });
    let skip = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.skip_this_update(cx);
    });
    let open_page = cx.listener(|this, _e: &gpui::ClickEvent, _w, _cx| {
        this.open_release_page();
    });

    // Release notes can be long, so size the card to a fraction of the window
    // (issue #29 comment): 0.8× viewport width, and the notes pane scrolls within
    // ~half the viewport height. The notes text is rendered at ~0.7× the current
    // UI zoom so more fits.
    let viewport = window.viewport_size();
    let card_w = px((f32::from(viewport.width) * 0.8).max(360.0));
    let notes_h = px((f32::from(viewport.height) * 0.5).max(160.0));
    let notes_font = px((theme::rem_size_px() * 0.7).max(9.0));

    let mut card =
        div()
            .w(card_w)
            .bg(rgb(current_theme().modal))
            .rounded_lg()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(render_modal_title_row(
                SharedString::from("Update available"),
                Some((
                    ModalIcon::Path("icons/refresh-cw.svg"),
                    current_theme().color_branch,
                )),
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_sm()
                    .child(div().text_color(rgb(current_theme().text_sub)).child(
                        SharedString::from(format!(
                            "v{}.{}.{}",
                            plan.current.major, plan.current.minor, plan.current.patch
                        )),
                    ))
                    .child(
                        div()
                            .text_color(rgb(current_theme().text_label))
                            .child(SharedString::from("\u{2192}")),
                    )
                    .child(
                        div()
                            .text_color(rgb(current_theme().text_main))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(SharedString::from(plan.tag.clone())),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_sub))
                    .child(SharedString::from(format!(
                        "{}  ({:.1} MB)",
                        plan.asset.name,
                        plan.asset.size as f64 / 1_048_576.0
                    ))),
            );

    // Release notes — rendered as Markdown (gpui-component TextView). The
    // TextView scrolls itself (`.scrollable(true)`) inside a fixed-height pane;
    // the earlier `overflow_y_scroll` wrapper did not actually scroll the async
    // TextView. Heading/body sizes are scaled to ~0.7× the UI zoom, and the
    // markdown style follows the active (dark) theme so code blocks render right.
    if !plan.notes.trim().is_empty() {
        use gpui_component::text::TextViewStyle;
        use gpui_component::ActiveTheme as _;

        let highlight_theme = cx.theme().highlight_theme.clone();
        let is_dark = cx.theme().mode.is_dark();
        let tv_style = TextViewStyle {
            heading_base_font_size: notes_font,
            highlight_theme,
            is_dark,
            ..Default::default()
        };

        card = card.child(
            div()
                .id("update-notes")
                .h(notes_h)
                .w_full()
                .p_2()
                .rounded_md()
                .bg(rgb(current_theme().bg_base))
                .text_color(rgb(current_theme().text_main))
                .text_size(notes_font)
                .child(
                    gpui_component::text::TextView::markdown(
                        "update-notes-md",
                        SharedString::from(plan.notes.clone()),
                    )
                    .scrollable(true)
                    .style(tv_style),
                ),
        );
    }

    if let Some(s) = status {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_warning))
                .child(s),
        );
    }

    // Buttons row.
    let mut actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            Button::new("update-skip")
                .label("Skip this version")
                .ghost()
                .small()
                .on_click(skip),
        )
        .child(
            Button::new("update-page")
                .label("Release page")
                .ghost()
                .small()
                .on_click(open_page),
        )
        .child(div().flex_grow(1.))
        .child(
            Button::new("update-cancel")
                .label("Later")
                .ghost()
                .small()
                .on_click(cancel),
        );
    if installing {
        actions = actions.child(
            Button::new("update-now")
                .label("Updating…")
                .primary()
                .small()
                .loading(true)
                .disabled(true)
                .on_click(|_, _, _| {}),
        );
    } else {
        actions = actions.child(
            Button::new("update-now")
                .label("Update now")
                .primary()
                .small()
                .on_click(update_now),
        );
    }
    card = card.child(actions);

    modal_overlay(card).into_any_element()
}

/// Editor Workspace unsaved-changes confirmation (T-WS-EDITOR-002 §5). Not a
/// Git write — no `OperationPlan`/current↔predicted card — just a plain
/// discard-or-cancel gate before switching file/source or closing the
/// workspace while its buffer is dirty. Enter/Esc come free from the
/// existing `confirm_active_modal`/`cancel_active_modal` root plumbing.
pub(crate) fn render_editor_dirty_guard_modal(
    _modal: EditorDirtyGuardModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_editor_dirty_guard();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let discard = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_editor_dirty_guard(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    let card = div()
        .w(theme::scaled_px(420.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(render_modal_title_row(
            SharedString::from(Msg::EditorWorkspaceUnsavedTitle.t()),
            Some((
                ModalIcon::Path("icons/trash-2.svg"),
                current_theme().color_blocker,
            )),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("editor-dirty-guard-cancel")
                        .label(Msg::EditorWorkspaceCancel.t())
                        .ghost()
                        .small()
                        .on_click(cancel),
                )
                .child(
                    KagiButton::accent(
                        "editor-dirty-guard-discard",
                        Msg::EditorWorkspaceDiscard.t(),
                        current_theme().color_blocker,
                        cx,
                    )
                    .small()
                    .on_click(discard),
                ),
        );

    modal_overlay(card).into_any_element()
}
