//! Smart Commit, update, editor guard, and app-notice modal renderers.

#![allow(clippy::too_many_arguments)]

use super::button_style::{modal_button, modal_button_without_tab_stop, ModalButtonKind};
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_modal_title_row, ModalIcon};
use super::modal_shell::{
    modal_body, modal_card, modal_card_sized, modal_scroll_body, MODAL_W_MD, MODAL_W_SM,
};
use super::modals::AppNotice;
use super::theme::{self, theme as current_theme};
use super::{smart_commit, KagiApp};
use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};
use gpui_component::IconName;

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
    app: &KagiApp,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let card =
        match modal {
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
                                    .child(SharedString::from(line.t())),
                            ),
                    );
                }
                // The short intro and four fixed `CONSENT_LINES` are bounded;
                // this card needs no scrolling body (#454).
                modal_card(MODAL_W_MD)
                    .child(div().flex_shrink_0().child(render_modal_title_row(
                        SharedString::from(Msg::SmartConsentTitle.t()),
                        Some((
                            ModalIcon::Path("icons/sparkles.svg"),
                            current_theme().color_success,
                        )),
                    )))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(current_theme().text_sub))
                            .child(SharedString::from(Msg::SmartConsentIntro.t())),
                    )
                    .child(lines_col)
                    .child(
                        div().flex_shrink_0().child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .justify_end()
                                .child(modal_button(
                                    "smart-consent-cancel",
                                    Msg::PlanCancel.t(),
                                    ModalButtonKind::Cancel,
                                    None,
                                    cancel,
                                    cx,
                                ))
                                .child(modal_button(
                                    "smart-consent-confirm",
                                    Msg::SmartConsentEnable.t(),
                                    ModalButtonKind::Primary,
                                    None,
                                    confirm,
                                    cx,
                                )),
                        ),
                    )
            }
            smart_commit::SmartCommitModal::ModelPicker { models } => {
                let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                    this.cancel_smart_modal(cx);
                });
                let state = &app.smart_model_focus;
                let scroll = super::keyboard_nav::RowScroll::List(state.scroll.clone());
                let rows = std::rc::Rc::new(state.focus.borrow_mut().rows(
                    state.keys.clone(),
                    &scroll,
                    app.root_focus.as_ref(),
                    window,
                    cx,
                ));
                if state.focus_first.replace(false) {
                    state.focus.borrow().focus_first(&scroll, window, cx);
                }
                let selected = state
                    .focus
                    .borrow()
                    .focused(window)
                    .and_then(|key| models.iter().position(|model| model == key));
                let size = models.len();
                let entity = cx.entity();
                let wrapper = rows.list(super::list_a11y::list_box(
                    "smart-model-list",
                    div().id("smart-model-list").w_full().min_h_0(),
                    Msg::SmartModelTitle.t(),
                ));
                let list = gpui::list(state.scroll.clone(), move |i, _window, _cx| {
                    let model = models[i].clone();
                    let entity = entity.clone();
                    rows.row(
                        i,
                        super::list_a11y::list_option(
                            "smart-model-list",
                            div().id(("smart-model", i)),
                            i,
                            size,
                            model.clone(),
                            selected == Some(i),
                        ),
                    )
                    .w_full()
                    .px(super::keyboard_nav::inset(12.))
                    .py(super::keyboard_nav::inset(4.))
                    .rounded_sm()
                    .bg(rgb(if selected == Some(i) {
                        current_theme().selected
                    } else {
                        current_theme().surface
                    }))
                    .text_sm()
                    .text_color(rgb(current_theme().text_main))
                    .on_click(move |_, window, cx| {
                        entity.update(cx, |app, cx| {
                            app.choose_smart_model(model.clone(), window, cx)
                        });
                    })
                    .hover(|s| s.bg(rgb(current_theme().selected)).cursor_pointer())
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .child(SharedString::from(models[i].clone())),
                    )
                    .into_any_element()
                })
                .w_full()
                .h(theme::scaled_px((size as f32 * 32.).min(280.)));
                modal_card(MODAL_W_SM)
                    .child(div().flex_shrink_0().child(render_modal_title_row(
                        SharedString::from(Msg::SmartModelTitle.t()),
                        Some((
                            ModalIcon::Path("icons/sparkles.svg"),
                            current_theme().color_branch,
                        )),
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_h_0()
                            .gap_2()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_sm()
                                    .text_color(rgb(current_theme().text_sub))
                                    .child(Msg::SmartModelRemembered.t()),
                            )
                            .child(wrapper.child(list)),
                    )
                    .child(div().flex_shrink_0().child(
                        div().flex().flex_row().justify_end().child(modal_button(
                            "smart-model-cancel",
                            Msg::PlanCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            cancel,
                            cx,
                        )),
                    ))
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
        this.cancel_update_modal();
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

    // The width is viewport-derived (0.8x the window), not a fixed layout
    // constant, so it uses the width-free shell: `modal_card`'s `scaled_px`
    // would apply the UI zoom on top of a width the viewport already
    // reflects. Everything else the shell sets (height cap, clipping,
    // padding, colours) applies.
    let card = modal_card_sized()
        .w(card_w)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(Msg::UpdateAvailableTitle.t()),
            Some((
                ModalIcon::Path("icons/refresh-cw.svg"),
                current_theme().color_branch,
            )),
        )));

    // The release-notes pane below carries its own scroller, so the body must
    // not scroll as well — one scroll region per card (#454). The short rows
    // are `flex_shrink_0`; the notes pane is the one element allowed to give
    // up height when the card hits its cap, and it scrolls what it clips.
    let mut body =
        modal_body()
            .child(
                div()
                    .flex_shrink_0()
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
                    .flex_shrink_0()
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

        body = body.child(
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
                        SharedString::from(kagi_ui_core::markdown::flatten_html_blocks(
                            &plan.notes,
                        )),
                    )
                    .plugin(kagi_ui_core::markdown::MarkdownImages::remote())
                    .scrollable(true)
                    .style(tv_style),
                ),
        );
    }

    if let Some(s) = status {
        body = body.child(super::e2e::measure_control(
            "update/status",
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_warning))
                .child(s),
        ));
    }

    // Installation is the only primary action; release notes and skip are quieter.
    let actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(modal_button(
            "update-skip",
            Msg::UpdateSkipVersion.t(),
            ModalButtonKind::Secondary,
            None,
            skip,
            cx,
        ))
        .child(modal_button(
            "update-page",
            Msg::UpdateReleasePage.t(),
            ModalButtonKind::Secondary,
            None,
            open_page,
            cx,
        ))
        .child(div().flex_grow(1.))
        .child(modal_button(
            "update-cancel",
            Msg::UpdateLater.t(),
            ModalButtonKind::Cancel,
            None,
            cancel,
            cx,
        ))
        .child(modal_button(
            "update-now",
            if installing {
                Msg::UpdateInstalling.t()
            } else {
                Msg::UpdateNow.t()
            },
            ModalButtonKind::Primary,
            installing.then(|| SharedString::from(Msg::UpdateInstallingReason.t())),
            update_now,
            cx,
        ));
    let card = card.child(body).child(div().flex_shrink_0().child(actions));

    // Measure the in-flow card, not the absolute overlay: a relative probe
    // around the overlay would give it a zero-height containing block.
    modal_overlay(super::e2e::measure_control("active-modal/update", card)).into_any_element()
}

/// Editor Workspace unsaved-changes confirmation (T-WS-EDITOR-002 §5). Not a
/// Git write — no `OperationPlan`/current↔predicted card — just a plain
/// discard-or-cancel gate before switching file/source or closing the
/// workspace while its buffer is dirty. Enter/Esc come free from the
/// existing `confirm_active_modal`/`cancel_active_modal` root plumbing.
pub(crate) fn render_editor_dirty_guard_modal(cx: &mut Context<KagiApp>) -> gpui::AnyElement {
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

    let card = modal_card(MODAL_W_SM)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(Msg::EditorWorkspaceUnsavedTitle.t()),
            Some((
                ModalIcon::Path("icons/file-text.svg"),
                current_theme().color_warning,
            )),
        )))
        .child(
            div().flex_shrink_0().child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .justify_end()
                    .child(modal_button(
                        "editor-dirty-guard-cancel",
                        Msg::EditorWorkspaceCancel.t(),
                        ModalButtonKind::Cancel,
                        None,
                        cancel,
                        cx,
                    ))
                    .child(modal_button(
                        "editor-dirty-guard-discard",
                        Msg::EditorWorkspaceDiscard.t(),
                        ModalButtonKind::Destructive,
                        None,
                        discard,
                        cx,
                    )),
            ),
        );

    modal_overlay(card)
        .child(super::e2e::measure_inside(
            "active-modal/editor-dirty-guard",
        ))
        .into_any_element()
}

/// AppNotice uses the shared card; its untyped message gets a neutral title.
/// Operation Log owns copying; the notice retains its existing action.
pub(crate) fn render_app_notice_modal(
    notice: AppNotice,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let action_label = if notice.inspect.is_some() {
        Msg::AppReconcileInspect.t()
    } else if let Some(read) = &notice.acknowledge {
        if read.can_acknowledge_unobserved() {
            if notice.release_armed {
                Msg::AppReconcileReleaseUnobservable.t()
            } else {
                Msg::AppReconcileArmRelease.t()
            }
        } else {
            Msg::AppReconcileConfirm.t()
        }
    } else {
        Msg::AppNoticeDismiss.t()
    };

    let card =
        modal_card(MODAL_W_SM)
            .child(div().flex_shrink_0().child(render_modal_title_row(
                SharedString::from(Msg::AppNoticeTitle.t()),
                Some((IconName::Info.into(), current_theme().color_warning)),
            )))
            .child(
                // One scroll region for the whole card: the message is producer
                // text of unbounded length (a multi-line evidence string among
                // them), and the header and the action row stay pinned.
                modal_scroll_body()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(current_theme().text_main))
                            .child(SharedString::from(notice.message)),
                    )
                    // #706 stage two. Never behind disclosure: this is the
                    // sentence the second confirm acts on.
                    .when(notice.release_armed, |body| {
                        body.child(super::e2e::measure_control(
                            "app-notice-release-warning",
                            div()
                                .text_sm()
                                .text_color(rgb(current_theme().color_warning))
                                .child(Msg::AppReconcileReleaseArmed.t()),
                        ))
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(current_theme().text_muted))
                            .child(Msg::AppNoticeDetailsInOpLog.t()),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .child(div().flex().flex_row().justify_end().child(
                        super::e2e::measure_control(
                            "app-notice-confirm",
                            modal_button_without_tab_stop(
                                "app-notice-dismiss",
                                action_label,
                                if notice.release_armed {
                                    ModalButtonKind::Destructive
                                } else {
                                    ModalButtonKind::Primary
                                },
                                None,
                                cx.listener(|this, _e: &gpui::ClickEvent, _, cx| {
                                    this.confirm_app_notice(cx);
                                }),
                                cx,
                            ),
                        ),
                    )),
            );

    modal_overlay(card)
        .child(super::e2e::measure_inside("active-modal/app-notice"))
        .into_any_element()
}
