//! Smart Commit, update, editor guard, and app-notice modal renderers.

#![allow(clippy::too_many_arguments)]

use super::button_style::{modal_button, modal_button_with_tab_stop, ModalButtonKind};
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_modal_title_row, ModalIcon};
use super::modal_shell::{
    modal_body, modal_card, modal_card_sized, modal_list_max_h, modal_scroll_body,
    MODAL_LIST_ROW_H, MODAL_W_MD, MODAL_W_SM,
};
use super::modals::AppNotice;
use super::theme::{self, theme as current_theme};
use super::{smart_commit, KagiApp};
use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};
use gpui_component::IconName;

// ──────────────────────────────────────────────────────────────
// Smart Commit modal renderer (T-COMMIT-016, ADR-0044)
// ──────────────────────────────────────────────────────────────

/// One privacy disclosure for Generate and both Settings hosts.
pub(super) fn smart_consent_card(
    confirm: super::dialog_a11y::DialogHandler,
    cancel: super::dialog_a11y::DialogHandler,
    cx: &gpui::App,
) -> gpui::Div {
    let mut lines = div().flex().flex_col().gap_1();
    for (index, line) in smart_commit::CONSENT_LINES.into_iter().enumerate() {
        let text = line.t();
        let row = div()
            .flex()
            .flex_row()
            .gap_1()
            .text_sm()
            .child(
                div()
                    .text_color(rgb(current_theme().color_branch))
                    .child("•"),
            )
            .child(
                div()
                    .min_w_0()
                    .text_color(rgb(current_theme().text_main))
                    .child(text),
            );
        #[cfg(feature = "gui-e2e")]
        let row = super::e2e::measure_control(format!("smart-consent-line-{index}-{text}"), row);
        #[cfg(not(feature = "gui-e2e"))]
        let _ = index;
        lines = lines.child(row);
    }
    modal_card(MODAL_W_MD)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            Msg::SmartConsentTitle.t().into(),
            Some((
                ModalIcon::Path("icons/sparkles.svg"),
                current_theme().color_success,
            )),
        )))
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_sub))
                .child(Msg::SmartConsentIntro.t()),
        )
        .child(lines)
        .child(
            div().flex_shrink_0().child(
                super::button_style::modal_button_row()
                    .child(super::e2e::measure_control(
                        "smart-consent-cancel",
                        modal_button(
                            "smart-consent-cancel",
                            Msg::PlanCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            move |_, window, cx| cancel(window, cx),
                            cx,
                        ),
                    ))
                    .child(super::e2e::measure_control(
                        "smart-consent-confirm",
                        modal_button(
                            "smart-consent-confirm",
                            Msg::SmartConsentEnable.t(),
                            ModalButtonKind::Primary,
                            None,
                            move |_, window, cx| confirm(window, cx),
                            cx,
                        ),
                    )),
            ),
        )
}

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
    let card = match modal {
        smart_commit::SmartCommitModal::Consent => {
            let owner = cx.entity().downgrade();
            let cancel: super::dialog_a11y::DialogHandler = std::rc::Rc::new(move |_, cx| {
                let _ = owner.update(cx, |app, cx| app.cancel_smart_modal(cx));
            });
            let owner = cx.entity().downgrade();
            let confirm: super::dialog_a11y::DialogHandler = std::rc::Rc::new(move |window, cx| {
                let _ = owner.update(cx, |app, cx| app.confirm_smart_consent(window, cx));
            });
            smart_consent_card(confirm, cancel, cx)
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
            let window_active = app.window_active;
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
                .h(theme::scaled_px(MODAL_LIST_ROW_H))
                .flex()
                .items_center()
                .px(super::keyboard_nav::inset(12.))
                .py_0()
                .rounded_sm()
                .bg(if selected == Some(i) {
                    current_theme().selected_for_window(window_active)
                } else {
                    rgb(current_theme().surface).into()
                })
                .text_xs()
                .line_height(
                    theme::scaled_px(MODAL_LIST_ROW_H) - px(2. * super::keyboard_nav::RING),
                )
                .text_color(
                    if current_theme().selection_uses_on_accent(selected == Some(i), window_active)
                    {
                        current_theme().selection_foreground(window_active)
                    } else {
                        rgb(current_theme().text_main).into()
                    },
                )
                .on_click(move |_, window, cx| {
                    entity.update(cx, |app, cx| {
                        app.choose_smart_model(model.clone(), window, cx)
                    });
                })
                .cursor_pointer()
                .when(selected != Some(i), |row| {
                    row.hover(|s| s.bg(rgb(current_theme().surface)))
                })
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .child(SharedString::from(models[i].clone())),
                )
                .into_any_element()
            })
            .w_full()
            .h(modal_list_max_h(size));
            modal_card(MODAL_W_SM)
                .child(div().flex_shrink_0().child(render_modal_title_row(
                    SharedString::from(Msg::SmartModelTitle.t()),
                    Some((
                        ModalIcon::Path("icons/sparkles.svg"),
                        current_theme().color_branch,
                    )),
                )))
                .child(
                    modal_body()
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
                .child(
                    div()
                        .flex_shrink_0()
                        .child(super::button_style::modal_button_row().child(modal_button(
                            "smart-model-cancel",
                            Msg::PlanCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            cancel,
                            cx,
                        ))),
                )
        }
    };

    modal_overlay(card)
}

/// Auto-update detail modal (ADR-0082, T-AUTOUPDATE-001).
///
/// Shows current → latest, the chosen asset and release notes. Installation
/// becomes an explicit Restart action while the installed update is pending.
pub(crate) fn render_update_modal(
    plan: kagi_domain::update::UpdatePlan,
    installing: bool,
    restart_pending: bool,
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

    let status = if restart_pending {
        Some(SharedString::from(Msg::UpdateRestartReady.t()))
    } else {
        status
    };
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

    // Restart replaces installation after success; skip cannot undo an installed update.
    let actions = super::button_style::modal_button_row()
        .items_center()
        .child(super::e2e::measure_control(
            "update-skip",
            modal_button(
                "update-skip",
                Msg::UpdateSkipVersion.t(),
                ModalButtonKind::Secondary,
                if installing {
                    Some(SharedString::from(Msg::UpdateInstallingReason.t()))
                } else {
                    restart_pending.then(|| SharedString::from(Msg::UpdateRestartReady.t()))
                },
                skip,
                cx,
            ),
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
        .child(super::e2e::measure_control(
            "update-cancel",
            modal_button(
                "update-cancel",
                if installing {
                    Msg::UpdateHideContinuing.t()
                } else if restart_pending {
                    Msg::EditorWorkspaceCancel.t()
                } else {
                    Msg::UpdateLater.t()
                },
                ModalButtonKind::Cancel,
                None,
                cancel,
                cx,
            ),
        ))
        .child(super::e2e::measure_control(
            "update-now",
            modal_button(
                "update-now",
                if installing {
                    Msg::UpdateInstalling.t()
                } else if restart_pending {
                    Msg::UpdateRestart.t()
                } else {
                    Msg::UpdateNow.t()
                },
                ModalButtonKind::Primary,
                installing.then(|| SharedString::from(Msg::UpdateInstallingReason.t())),
                update_now,
                cx,
            ),
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
pub(crate) fn render_editor_dirty_guard_modal(
    host_close: bool,
    count: usize,
    saving: bool,
    restart_waiting: bool,
    restart_message: Msg,
    conflict_guard: Option<(bool, bool)>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let conflict_present = conflict_guard.is_some();
    let (conflict_failed, conflict_discard_armed) = conflict_guard.unwrap_or((false, false));
    let title = if conflict_failed {
        Msg::ConflictDraftSaveFailed.t()
    } else if restart_waiting {
        Msg::UpdateRestart.t()
    } else if host_close {
        Msg::EditorHostUnsavedTitle.t()
    } else {
        Msg::EditorWorkspaceUnsavedTitle.t()
    };
    let retry_label = if conflict_failed {
        Msg::ConflictDraftRetry.t()
    } else if !host_close {
        Msg::EditorSave.t()
    } else {
        Msg::EditorHostSaveAll.t()
    };
    let discard_label = if conflict_present {
        if conflict_discard_armed {
            Msg::ConflictDraftDiscardConfirm.t()
        } else {
            Msg::ConflictDraftDiscardClose.t()
        }
    } else if restart_waiting {
        Msg::UpdateRestart.t()
    } else {
        Msg::EditorWorkspaceDiscard.t()
    };
    let detail = if conflict_discard_armed && count > 0 {
        Msg::ConflictDraftMixedDiscardDetail.t()
    } else if conflict_discard_armed {
        Msg::ConflictDraftDiscardDetail.t()
    } else if conflict_failed {
        Msg::ConflictDraftUnsavedDetail.t()
    } else {
        Msg::ConflictDraftSaving.t()
    };
    #[cfg(feature = "gui-e2e")]
    if conflict_present {
        super::e2e::record_conflict_draft_guard(super::e2e::ConflictDraftGuardPresentation {
            title,
            retry: retry_label,
            discard: discard_label,
            detail,
            editor_count: count,
            saving,
        });
    }
    let busy = saving.then(|| SharedString::from(Msg::OpInProgress.t()));
    let save = cx.listener(move |this, _e: &gpui::ClickEvent, _, cx| {
        if conflict_failed {
            this.flush_host_conflict_drafts(cx);
        } else {
            this.save_host_dirty_guard(cx);
        }
    });
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
            SharedString::from(title),
            Some((
                ModalIcon::Path("icons/file-text.svg"),
                current_theme().color_warning,
            )),
        )))
        .when(conflict_present, |card| card.child(div().child(detail)))
        .when(restart_waiting, |card| {
            card.child(div().child(restart_message.t()))
        })
        .when(count > 0 && (host_close || conflict_present), |card| {
            card.child(
                div().child(
                    Msg::EditorHostUnsavedCount
                        .t()
                        .replace("{count}", &count.to_string()),
                ),
            )
        })
        .child(
            div().flex_shrink_0().child(
                super::button_style::modal_button_row()
                    .child(super::e2e::measure_control(
                        "editor-dirty-guard-cancel",
                        modal_button(
                            "editor-dirty-guard-cancel",
                            Msg::EditorWorkspaceCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            cancel,
                            cx,
                        ),
                    ))
                    .child(super::e2e::measure_control(
                        "editor-dirty-guard-discard",
                        modal_button(
                            "editor-dirty-guard-discard",
                            discard_label,
                            if restart_waiting {
                                ModalButtonKind::Primary
                            } else {
                                ModalButtonKind::Destructive
                            },
                            busy.clone(),
                            discard,
                            cx,
                        ),
                    ))
                    .when(
                        (host_close || conflict_present) && !restart_waiting,
                        |row| {
                            row.child(super::e2e::measure_control(
                                "editor-dirty-guard-save",
                                modal_button(
                                    "editor-dirty-guard-save",
                                    retry_label,
                                    ModalButtonKind::Primary,
                                    busy,
                                    save,
                                    cx,
                                ),
                            ))
                        },
                    ),
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

    let card = modal_card(MODAL_W_SM)
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
                .child(
                    super::button_style::modal_button_row().child(super::e2e::measure_control(
                        "app-notice-confirm",
                        modal_button_with_tab_stop(
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
                            notice.release_armed,
                            cx,
                        ),
                    )),
                ),
        );

    modal_overlay(card)
        .child(super::e2e::measure_inside("active-modal/app-notice"))
        .into_any_element()
}
