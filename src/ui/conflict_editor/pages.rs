//! Hunk decision and review chrome; code rows and result rendering stay shared.
use super::*;
pub(super) fn render_toolbar(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let path_str = path.to_string_lossy().into_owned();
    let total = mode.buffer.hunk_count(path);
    let n_of_m = format!(
        "{} {}/{}",
        Msg::EditorConflictNofM.t(),
        chrome.selected_hunk + 1,
        total
    );

    let prev = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_select_hunk(this.selected_hunk.saturating_sub(1));
        cx.notify();
    });
    let next = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        let total = this
            .editing
            .as_deref()
            .and_then(|path| this.mode.as_ref().map(|mode| mode.buffer.hunk_count(path)))
            .unwrap_or(0);
        if this.selected_hunk + 1 < total {
            this.conflict_editor_select_hunk(this.selected_hunk + 1);
        } else {
            this.conflict_show_page(ConflictPage::Review, cx);
            this.focus_navigation(window, cx);
        }
        cx.notify();
    });
    let p_reset = path.to_path_buf();
    let reset_focus = chrome.reset_focus.clone();
    let reset = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        if let Some(focus) = &reset_focus {
            if !focus.contains_focused(window, cx) {
                focus.focus(window, cx);
            }
        }
        this.conflict_editor_reset_all_request(&p_reset);
        cx.notify();
    });
    let reset_armed = chrome.reset_all_armed;

    div()
        .flex()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .flex_wrap()
        .gap_2()
        .w_full()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(6.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().color_warning))
        .child(super::e2e::measure_control(
            "conflict-overview",
            tool_button(
                "editor-files",
                Msg::ConflictFiles.t(),
                ButtonRole::NeutralTinted,
                cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                    view.conflict_show_page(ConflictPage::Overview, cx);
                    view.focus_navigation(window, cx);
                    cx.notify();
                }),
                cx,
            ),
        ))
        .child(
            // file name + conflict n/m laid out horizontally (was stacked).
            div()
                .flex_grow(1.)
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .child(SharedString::from(path_str)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::scaled_px(10.))
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(n_of_m)),
                ),
        )
        .when(chrome.page == ConflictPage::Hunk, |el| {
            el.child(super::e2e::measure_control(
                "conflict-editor-prev",
                tool_button(
                    "editor-prev",
                    Msg::EditorPrevHunk.t(),
                    ButtonRole::NeutralTinted,
                    prev,
                    cx,
                )
                .disabled(chrome.selected_hunk == 0),
            ))
            .child(super::e2e::measure_control(
                if chrome.selected_hunk + 1 < total {
                    "conflict-editor-next"
                } else {
                    "conflict-review"
                },
                tool_button(
                    "editor-next",
                    if chrome.selected_hunk + 1 < total {
                        Msg::EditorNextHunk.t()
                    } else {
                        Msg::ConflictReview.t()
                    },
                    ButtonRole::Primary,
                    next,
                    cx,
                ),
            ))
        })
        .when(chrome.page == ConflictPage::Review, |el| {
            el.child(super::e2e::measure_control(
                "conflict-review",
                tool_button(
                    "editor-review",
                    Msg::ConflictBackToHunk.t(),
                    ButtonRole::NeutralTinted,
                    cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                        view.conflict_show_page(ConflictPage::Hunk, cx);
                        view.focus_navigation(window, cx);
                        cx.notify();
                    }),
                    cx,
                ),
            ))
        })
        // Reset all — destructive: trash icon, armed → blocker colour + confirm label.
        .child(super::e2e::measure_control(
            "conflict-editor-reset",
            div()
                .when_some(chrome.reset_focus.as_ref(), |el, focus| {
                    el.track_focus(focus)
                })
                .child(
                    icon_button(
                        "editor-reset",
                        "icons/trash-2.svg",
                        if reset_armed {
                            Msg::EditorResetAllConfirm.t()
                        } else {
                            Msg::EditorReset.t()
                        },
                        if reset_armed {
                            ButtonRole::Danger
                        } else {
                            ButtonRole::Warning
                        },
                        reset,
                        cx,
                    )
                    .disabled(chrome.writer_busy)
                    .when(cfg!(feature = "gui-e2e"), |button| {
                        button.on_key_down(|event, _, _| {
                            #[cfg(feature = "gui-e2e")]
                            if event.keystroke.key == "f19" {
                                RESET_BUTTON_FOCUSED.with(|focused| focused.set(true));
                            }
                            #[cfg(not(feature = "gui-e2e"))]
                            let _ = event;
                        })
                    }),
                ),
        ))
        .into_any_element()
}
pub(super) fn tool_button<H>(
    id: &str,
    label: &str,
    role: ButtonRole,
    handler: H,
    cx: &gpui::App,
) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    KagiButton::styled(
        SharedString::from(id.to_string()),
        SharedString::from(label.to_string()),
        role,
        cx,
    )
    .on_click(handler)
}
/// An icon button with a compact text label beside the glyph (POLISH-040/041).
fn icon_button<H>(
    id: &str,
    icon_path: &'static str,
    label: &str,
    role: ButtonRole,
    handler: H,
    cx: &gpui::App,
) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    KagiButton::icon(
        SharedString::from(id.to_string()),
        icon_path,
        SharedString::from(label.to_string()),
        role,
        cx,
    )
    .small()
    .on_click(handler)
}
pub(super) fn render_panes(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    // No hunk model (binary / single-sided) → guidance message.
    let Some(_inputs) = chrome.inputs.as_ref().filter(|i| i.path == path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };
    let Some(model) = mode.buffer.hunk_model(path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };

    let labels = mode.labels();
    chrome.resolver_keyboard.borrow_mut().side_names = [
        Msg::conflict_side(&labels.current.role, &labels.current.name),
        Msg::conflict_side(&labels.incoming.role, &labels.incoming.name),
    ];
    let current_label = format!("● {} — {}", Msg::EditorCurrentSide.t(), labels.current.name);
    let incoming_label = format!(
        "○ {} — {}",
        Msg::EditorIncomingSide.t(),
        labels.incoming.name
    );

    // ── A | B row (resizable A|B), measured for the vertical divider drag ──
    let ab_geom = chrome.ab_geom.clone();
    let ab_measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                kagi_ui_core::klog!(
                    "ab_geom left={:.1} right={:.1} width={:.1}",
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.x + bounds.size.width),
                    f32::from(bounds.size.width)
                );
            }
            ab_geom.set((
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.x + bounds.size.width),
            ));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    let scroll = chrome.ab_scroll.clone();
    let a_pane = pane(
        "conflict-pane-a",
        current_label,
        theme().color_branch,
        None,
        side_row_list(
            path,
            model,
            SelectionSide::Current,
            scroll.clone(),
            chrome.selected_hunk,
            chrome,
            cx,
        ),
    );

    let b_pane = pane(
        "conflict-pane-b",
        incoming_label,
        theme().color_remote,
        None,
        side_row_list(
            path,
            model,
            SelectionSide::Incoming,
            scroll,
            chrome.selected_hunk,
            chrome,
            cx,
        ),
    );

    // A definite content-sized height keeps virtual rows visible without a
    // half-screen blank reservation for small decisions. Large hunks scroll.
    let visible_rows = visible_hunk_rows(model, chrome.selected_hunk, chrome.show_context).len();
    let source_height = (visible_rows as f32 * 17. + 36.).clamp(75., 240.);
    let p_both = path.to_path_buf();
    let selected_hunk = chrome.selected_hunk;
    let ab_row = div()
        .relative()
        .flex()
        .flex_row()
        .w_full()
        .h(theme::scaled_px(source_height))
        .flex_shrink_0()
        // Measure the FULL A·B row width (not just the A pane) so the divider
        // drag maps the cursor against the whole span — measuring inside A would
        // shrink the span and feed back on itself, making resize unusable.
        .child(ab_measure)
        .child(
            div()
                .h_full()
                .min_w(px(0.))
                .w(relative(chrome.ab_split))
                .child(a_pane),
        )
        .child(vertical_divider())
        .child(div().h_full().min_w(px(0.)).flex_1().child(b_pane));

    let source_actions = div()
        .flex()
        .flex_wrap()
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .p(theme::scaled_px(6.))
        .child(super::e2e::measure_control(
            "conflict-both",
            tool_button(
                "editor-both",
                Msg::ConflictChooseBoth.t(),
                ButtonRole::Secondary,
                cx.listener(move |view, _: &gpui::ClickEvent, _, cx| {
                    view.conflict_choose_both(&p_both, selected_hunk, cx);
                    cx.notify();
                }),
                cx,
            )
            .disabled(!chrome.source_choices_enabled),
        ))
        .child(super::e2e::measure_control(
            "conflict-context",
            tool_button(
                "editor-context",
                Msg::ConflictContext.t(),
                if chrome.show_context {
                    ButtonRole::Secondary
                } else {
                    ButtonRole::NeutralTinted
                },
                cx.listener(|view, _: &gpui::ClickEvent, _, cx| {
                    view.show_context = !view.show_context;
                    cx.notify();
                }),
                cx,
            ),
        ));

    div()
        .id("conflict-decision-scroll")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(source_actions)
        .child(ab_row)
        .child(render_hunk_result(mode, chrome, path, cx))
        .into_any_element()
}
fn render_hunk_result(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let Some(model) = mode.buffer.hunk_model(path) else {
        return div().into_any_element();
    };
    let manual = chrome.manual_override;
    let hunk = model
        .regions
        .iter()
        .filter_map(|region| match region {
            Region::Hunk(hunk) => Some(hunk),
            _ => None,
        })
        .nth(chrome.selected_hunk);
    let lines = hunk
        .filter(|hunk| !manual && hunk.is_resolved())
        .map(|hunk| hunk.resolved_lines());
    let result_rows = lines.as_ref().map_or(1, Vec::len).clamp(1, 8);
    let mut result = div()
        .id("conflict-hunk-result")
        .flex()
        .flex_col()
        .h(theme::scaled_px(result_rows as f32 * 17. + 60.))
        .flex_shrink_0()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .flex_shrink_0()
                .gap_2()
                .p(theme::scaled_px(6.))
                .child(Msg::ConflictHunkResult.t())
                .child(
                    div()
                        .text_size(theme::scaled_px(11.))
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(flow::draft_label(mode, path))),
                )
                .child(super::e2e::measure_control(
                    "conflict-edit-result",
                    tool_button(
                        "hunk-edit-result",
                        Msg::ConflictEditResult.t(),
                        ButtonRole::Secondary,
                        cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                            view.conflict_edit_result(window, cx);
                        }),
                        cx,
                    )
                    .disabled(chrome.writer_busy),
                )),
        );
    if manual {
        result = result.child(
            div()
                .px(theme::scaled_px(8.))
                .text_color(rgb(theme().text_sub))
                .child(Msg::ConflictManualResult.t()),
        );
    } else if let Some(lines) = lines {
        if lines.is_empty() {
            result = result.child(Msg::ConflictEmptyResult.t());
        } else {
            result = result.child(render_result_preview(lines, chrome, path, cx));
        }
    } else {
        result = result.child(div().p(theme::scaled_px(8.)).child("—"));
    }
    result.into_any_element()
}
