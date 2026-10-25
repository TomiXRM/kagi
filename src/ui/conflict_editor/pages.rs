//! Borderless source documents, aligned decision gutter, and direct Result editor.
use super::*;

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
    .small()
    .h(theme::scaled_px(20.))
    .rounded(theme::scaled_px(5.))
    .text_size(theme::scaled_px(12.))
    .on_click(handler)
}

pub(super) fn render_panes(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    let Some(model) = mode.buffer.hunk_model(path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };
    let labels = mode.labels();
    chrome.resolver_keyboard.borrow_mut().side_names = [
        Msg::conflict_side(&labels.current.role, &labels.current.name),
        Msg::conflict_side(&labels.incoming.role, &labels.incoming.name),
    ];
    let a = pane(
        "conflict-pane-a",
        side_row_list(
            path,
            model,
            SelectionSide::Current,
            chrome.ab_scroll.clone(),
            chrome,
            cx,
        ),
    );
    let b = pane(
        "conflict-pane-b",
        side_row_list(
            path,
            model,
            SelectionSide::Incoming,
            chrome.ab_scroll.clone(),
            chrome,
            cx,
        ),
    );
    let total = mode.buffer.hunk_count(path);
    let selected = chrome.selected_hunk;
    let footer = |label: String| {
        div()
            .h(theme::scaled_px(25.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(theme::scaled_px(12.))
            .gap(theme::scaled_px(6.))
            .bg(rgb(theme().panel))
            .border_t_1()
            .border_color(rgb(theme().surface))
            .child(
                div()
                    .min_w(px(0.))
                    .flex_1()
                    .truncate()
                    .text_size(theme::scaled_px(11.))
                    .text_color(rgb(theme().text_sub))
                    .child(label),
            )
    };
    let previous = tool_button(
        "conflict-prev-hunk",
        "‹",
        ButtonRole::Neutral,
        cx.listener(|view, _, window, cx| {
            view.conflict_editor_select_hunk(view.selected_hunk.saturating_sub(1));
            view.focus_navigation(window, cx);
            cx.notify();
        }),
        cx,
    )
    .disabled(selected == 0)
    .aria_label(Msg::EditorPrevHunk.t());
    let next = tool_button(
        "conflict-next-hunk",
        "›",
        ButtonRole::Neutral,
        cx.listener(move |view, _, window, cx| {
            if view.selected_hunk + 1 < total {
                view.conflict_editor_select_hunk(view.selected_hunk + 1);
            }
            view.focus_navigation(window, cx);
            cx.notify();
        }),
        cx,
    )
    .disabled(selected + 1 >= total)
    .aria_label(Msg::EditorNextHunk.t());
    let left_footer = footer(if chrome.current_commit.is_empty() {
        labels.current.name
    } else {
        format!("{} · {}", labels.current.name, chrome.current_commit)
    })
    .child(e2e::measure_control("conflict-prev-hunk", previous))
    .child(
        div()
            .text_size(theme::scaled_px(11.))
            .text_color(rgb(theme().text_sub))
            .child(format!("{}/{total}", selected + 1)),
    )
    .child(e2e::measure_control("conflict-next-hunk", next));
    let gutter = decision_gutter(model, chrome, cx);
    div()
        .id("conflict-decision-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(
            div()
                .flex()
                .w_full()
                .flex_grow(2.)
                .flex_basis(px(0.))
                .min_h(theme::scaled_px(105.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(a)
                        .child(e2e::measure_control("resolver-current-footer", left_footer)),
                )
                .child(
                    div()
                        .w(theme::scaled_px(12.))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .bg(rgb(theme().panel))
                        .child(gutter)
                        .child(div().h(theme::scaled_px(25.)).flex_shrink_0()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(b)
                        .child(e2e::measure_control(
                            "resolver-incoming-footer",
                            footer(labels.incoming.name),
                        )),
                ),
        )
        .child(render_result_pane(mode, chrome, path, cx))
        .into_any_element()
}

fn decision_gutter(
    model: &kagi_git::resolution::HunkModel,
    chrome: &EditorChrome,
    cx: &mut Context<ConflictView>,
) -> impl IntoElement {
    let mut rows = Vec::new();
    let mut hunk = 0;
    for region in &model.regions {
        match region {
            Region::Passthrough(lines) => rows.extend((0..lines.len()).map(|_| None)),
            Region::Hunk(conflict) => {
                let band = Some((hunk, conflict.is_resolved()));
                rows.extend(
                    (0..1 + conflict.current.len().max(conflict.incoming.len()).max(1))
                        .map(|_| band),
                );
                hunk += 1;
            }
        }
    }
    #[cfg(feature = "gui-e2e")]
    crate::ui::conflict_editor_a11y::record_row_count("gutter", rows.len());
    let focused = chrome.selected_hunk;
    div()
        .relative()
        .flex_1()
        .min_h(px(0.))
        .child(e2e::measure_inside("resolver-gutter-viewport"))
        .child(
            uniform_list(
                "conflict-decision-gutter",
                rows.len(),
                cx.processor(move |_, range: Range<usize>, _, _| {
                    range
                        .filter_map(|i| rows.get(i).map(|row| (i, row)))
                        .map(|(i, row)| {
                            let hunk = row.map(|(hunk, _)| hunk);
                            div()
                                .id(SharedString::from(format!("gutter-{i}")))
                                .relative()
                                .h(theme::scaled_px(24.))
                                .w_full()
                                .border_l(theme::scaled_px(2.))
                                .border_color(if hunk == Some(focused) {
                                    rgb(theme().ui_accent()).into()
                                } else {
                                    gpui::transparent_black()
                                })
                                .when_some(*row, |el, (_, resolved)| {
                                    el.bg(rgb(if resolved {
                                        theme().diff_added_bg
                                    } else {
                                        theme().diff_removed_bg
                                    }))
                                })
                                .child(e2e::measure_inside(format!("resolver-gutter-row-{i}")))
                                .into_any_element()
                        })
                        .collect()
                }),
            )
            .track_scroll(&chrome.ab_scroll)
            .h_full()
            .w_full(),
        )
}
