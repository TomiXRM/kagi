//! Borderless source documents, aligned decision gutter, and direct Result editor.
use super::*;
use kagi_git::resolution::HunkChoice;
use kagi_ui_core::tooltip::HoverTooltip as _;

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
        format!("{} — {}", labels.current.name, Msg::EditorCurrentSide.t()),
        theme().color_branch,
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
        format!("{} — {}", labels.incoming.name, Msg::EditorIncomingSide.t()),
        theme().color_remote,
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
    let gutter = decision_gutter(model, path, chrome, cx);
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
                        .w(theme::scaled_px(78.))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .border_x_1()
                        .border_color(rgb(theme().surface))
                        .bg(rgb(theme().panel))
                        .child(div().h(theme::scaled_px(26.)).flex_shrink_0())
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
    path: &std::path::Path,
    chrome: &EditorChrome,
    cx: &mut Context<ConflictView>,
) -> impl IntoElement {
    let mut rows = Vec::new();
    let mut hunk = 0;
    for region in &model.regions {
        match region {
            Region::Passthrough(lines) => rows.extend((0..lines.len()).map(|_| None)),
            Region::Hunk(conflict) => {
                let left = conflict.side_state(SelectionSide::Current);
                let right = conflict.side_state(SelectionSide::Incoming);
                let mixed = left == TriState::Partial || right == TriState::Partial;
                let states = if mixed {
                    [TriState::Partial; 2]
                } else {
                    [left, right]
                };
                rows.push(Some((hunk, states, true)));
                rows.extend(
                    (1..conflict.current.len().max(conflict.incoming.len()).max(1))
                        .map(|_| Some((hunk, states, false))),
                );
                hunk += 1;
            }
        }
    }
    let path = path.to_path_buf();
    let enabled = chrome.source_choices_enabled;
    let menu_enabled = !chrome.writer_busy;
    let scroll = chrome.ab_scroll.clone();
    div().relative().flex_1().min_h(px(0.)).child(
        uniform_list(
            "conflict-decision-gutter",
            rows.len(),
            cx.processor(move |view, range: Range<usize>, _, cx| {
                view.resolver_keyboard.borrow_mut().drawn = range.clone();
                range
                    .filter_map(|i| rows.get(i).map(|row| (i, row)))
                    .map(|(i, row)| {
                        let band = div()
                            .id(SharedString::from(format!("gutter-{i}")))
                            .h(theme::scaled_px(24.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(row.is_some(), |el| el.bg(rgb(theme().diff_removed_bg)));
                        if let Some((hunk, states, true)) = row {
                            let hunk = *hunk;
                            let mut band = band;
                            for (key, label, name, choice, state) in [
                                (
                                    "left",
                                    "←",
                                    Msg::ConflictUseLeftSide,
                                    Some(HunkChoice::AcceptCurrent),
                                    states[0],
                                ),
                                ("more", "…", Msg::ConflictMoreChoices, None, TriState::None),
                                (
                                    "right",
                                    "→",
                                    Msg::ConflictUseRightSide,
                                    Some(HunkChoice::AcceptIncoming),
                                    states[1],
                                ),
                            ] {
                                let id = format!("conflict-hunk-{hunk}-{key}");
                                let path = path.clone();
                                let control = crate::ui::conflict_editor_keys::adoption(
                                    id,
                                    name.t().to_owned(),
                                    crate::ui::conflict_editor_keys::ControlState::Adoption(state),
                                    label,
                                    Some(hunk),
                                    if key == "more" { menu_enabled } else { enabled },
                                    &view.resolver_keyboard,
                                    cx.listener(
                                        move |view, event: &gpui::ClickEvent, window, cx| {
                                            if view.editing.as_deref() != Some(path.as_path()) {
                                                return;
                                            }
                                            view.conflict_editor_select_hunk(hunk);
                                            if choice.is_some() {
                                                view.resolver_keyboard
                                                    .borrow()
                                                    .focus_hunk(hunk, window, cx);
                                            } else {
                                                view.resolver_keyboard
                                                    .borrow()
                                                    .focus_more(hunk, window, cx);
                                            }
                                            if let Some(choice) = &choice {
                                                view.conflict_choose_hunk(
                                                    &path,
                                                    hunk,
                                                    choice.clone(),
                                                    cx,
                                                );
                                            } else {
                                                view.choice_menu = Some((hunk, event.position()));
                                            }
                                            cx.notify();
                                        },
                                    ),
                                    cx,
                                )
                                .size(theme::scaled_px(24.))
                                .rounded(theme::scaled_px(5.))
                                .text_size(theme::scaled_px(16.))
                                .text_color(rgb(theme().ui_accent()))
                                .when(state == TriState::All, |el| el.bg(rgb(theme().selected)))
                                .when(
                                    state == TriState::Partial,
                                    |el| {
                                        el.bg(gpui::rgba((theme().ui_accent() << 8) | 0x28))
                                            .border_b_2()
                                            .border_color(rgb(theme().ui_accent()))
                                    },
                                );
                                band = band.child(control.hover_tooltip(
                                    SharedString::from(format!("conflict-hunk-{hunk}-{key}-help")),
                                    name.t(),
                                ));
                            }
                            band.into_any_element()
                        } else {
                            band.into_any_element()
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(&scroll)
        .h_full()
        .w_full(),
    )
}
