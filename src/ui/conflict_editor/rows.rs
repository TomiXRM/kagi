//! Whole-file source rows; line selection remains the fine-grained decision tool.
use super::*;

#[derive(Clone)]
pub(super) enum SideRow {
    Action {
        hunk_index: usize,
    },
    Line {
        hunk_index: usize,
        line_index: usize,
        line_no: usize,
        text: String,
        taken: bool,
        diff_spans: Arc<Vec<Range<usize>>>,
    },
    Context {
        line_no: usize,
        text: String,
    },
    Blank {
        hunk_index: usize,
    },
}

fn intra_line_spans(text: &str, other: &str, side: SelectionSide) -> Vec<Range<usize>> {
    use kagi_domain::word_diff::{word_diff, Side};
    let (old, new, want) = match side {
        SelectionSide::Current => (text, other, Side::Old),
        SelectionSide::Incoming => (other, text, Side::New),
    };
    word_diff(old, new)
        .into_iter()
        .filter(|span| span.side == want)
        .map(|span| span.range)
        .collect()
}

pub(super) fn build_side_rows(
    model: &kagi_git::resolution::HunkModel,
    side: SelectionSide,
) -> Vec<SideRow> {
    let mut rows = Vec::new();
    let (mut hunk_index, mut line_no) = (0, 1);
    for region in &model.regions {
        match region {
            Region::Passthrough(lines) => {
                for text in lines {
                    rows.push(SideRow::Context {
                        line_no,
                        text: text.clone(),
                    });
                    line_no += 1;
                }
            }
            Region::Hunk(hunk) => {
                rows.push(SideRow::Action { hunk_index });
                let (lines, other, taken) = match side {
                    SelectionSide::Current => (
                        &hunk.current,
                        &hunk.incoming,
                        hunk.line_select
                            .as_ref()
                            .map(|s| s.taken(SelectionSide::Current)),
                    ),
                    SelectionSide::Incoming => (
                        &hunk.incoming,
                        &hunk.current,
                        hunk.line_select
                            .as_ref()
                            .map(|s| s.taken(SelectionSide::Incoming)),
                    ),
                };
                for (line_index, text) in lines.iter().enumerate() {
                    let is_taken = taken
                        .and_then(|values| values.get(line_index))
                        .copied()
                        .unwrap_or_else(|| hunk.side_state(side) == TriState::All);
                    rows.push(SideRow::Line {
                        hunk_index,
                        line_index,
                        line_no,
                        text: text.clone(),
                        taken: is_taken,
                        diff_spans: Arc::new(intra_line_spans(
                            text,
                            other.get(line_index).map(String::as_str).unwrap_or(""),
                            side,
                        )),
                    });
                    line_no += 1;
                }
                for _ in lines.len()..hunk.current.len().max(hunk.incoming.len()).max(1) {
                    rows.push(SideRow::Blank { hunk_index });
                }
                hunk_index += 1;
            }
        }
    }
    rows
}

pub(super) fn side_key(side: SelectionSide) -> &'static str {
    match side {
        SelectionSide::Current => "current",
        SelectionSide::Incoming => "incoming",
    }
}

pub(super) fn side_row_list(
    path: &std::path::Path,
    model: &kagi_git::resolution::HunkModel,
    side: SelectionSide,
    scroll: UniformListScrollHandle,
    chrome: &EditorChrome,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let rows = Arc::new(build_side_rows(model, side));
    let highlights = side_highlights(chrome, path, model, side);
    let enabled = chrome.source_choices_enabled;
    let reset_enabled = enabled;
    let bands: Arc<Vec<_>> = Arc::new(
        model
            .regions
            .iter()
            .filter_map(|region| match region {
                Region::Hunk(hunk) => Some((hunk.is_resolved(), hunk.side_state(side))),
                _ => None,
            })
            .collect(),
    );
    let focused = chrome.selected_hunk;
    #[cfg(feature = "gui-e2e")]
    crate::ui::conflict_editor_a11y::record_row_count(side_key(side), rows.len());
    let path = Arc::new(path.to_path_buf());
    let (id, outer) = match side {
        SelectionSide::Current => ("conflict-current-lines", "conflict-current-lines-scroll"),
        SelectionSide::Incoming => ("conflict-incoming-lines", "conflict-incoming-lines-scroll"),
    };
    div()
        .id(outer)
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_x_scroll()
        .child(e2e::measure_inside(format!(
            "resolver-{}-viewport",
            side_key(side)
        )))
        .child(
            uniform_list(
                id,
                rows.len(),
                cx.processor(move |view, range: Range<usize>, _, cx| {
                    view.resolver_keyboard.borrow_mut().drawn = range.clone();
                    range
                        .filter_map(|i| rows.get(i).map(|row| (i, row)))
                        .map(|(i, row)| {
                            let focused_row = matches!(row,
                                SideRow::Action { hunk_index } | SideRow::Blank { hunk_index }
                                | SideRow::Line { hunk_index, .. } if *hunk_index == focused);
                            let content = render_source_row(
                                i,
                                row,
                                highlights.get(i).cloned().unwrap_or_default(),
                                &path,
                                side,
                                enabled,
                                reset_enabled,
                                &view.resolver_keyboard,
                                &bands,
                                focused,
                                cx,
                            );
                            div()
                                .relative()
                                .h(theme::scaled_px(24.))
                                .min_w(relative(1.))
                                .child(content)
                                .child(e2e::measure_inside(format!(
                                    "resolver-{}-row-{i}",
                                    side_key(side)
                                )))
                                .when(focused_row, |el| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .w(theme::scaled_px(3.))
                                            .h_full()
                                            .bg(rgb(theme().ui_accent())),
                                    )
                                })
                                .into_any_element()
                        })
                        .collect()
                }),
            )
            .track_scroll(&scroll)
            .flex_1()
            .min_h(px(0.)),
        )
        .child(Scrollbar::vertical(&scroll))
}

fn line_number(line_no: usize) -> gpui::Div {
    div()
        .flex_shrink_0()
        .w(theme::scaled_px(17.))
        .pl(theme::scaled_px(2.))
        .text_size(theme::scaled_px(11.))
        .line_height(theme::scaled_px(24.))
        .font_family(terminal::pick_font_family())
        .text_color(rgb(theme().text_sub))
        .child(line_no.to_string())
}

#[allow(clippy::too_many_arguments)]
fn render_source_row(
    row_index: usize,
    row: &SideRow,
    highlights: RowHl,
    path: &Arc<std::path::PathBuf>,
    side: SelectionSide,
    enabled: bool,
    reset_enabled: bool,
    keyboard: &std::rc::Rc<std::cell::RefCell<crate::ui::conflict_editor_keys::ResolverKeyboard>>,
    bands: &[(bool, TriState)],
    focused: usize,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let hunk = match row {
        SideRow::Action { hunk_index }
        | SideRow::Blank { hunk_index }
        | SideRow::Line { hunk_index, .. } => Some(*hunk_index),
        SideRow::Context { .. } => None,
    };
    let (resolved, state) = hunk
        .map(|hunk| bands[hunk])
        .unwrap_or((false, TriState::None));
    let background = if !resolved {
        theme().diff_removed_bg
    } else if state == TriState::None {
        theme().surface
    } else {
        theme().diff_added_bg
    };
    let base = div()
        .flex()
        .items_center()
        .min_w(relative(1.))
        .h(theme::scaled_px(24.))
        .border_l(theme::scaled_px(3.))
        .border_color(if hunk == Some(focused) {
            rgb(theme().ui_accent()).into()
        } else {
            gpui::transparent_black()
        })
        .relative()
        .when(hunk.is_some(), |el| el.bg(rgb(background)));
    match row {
        SideRow::Action { hunk_index } => render_action_row(
            base,
            *hunk_index,
            path,
            side,
            resolved,
            state,
            background,
            if resolved { reset_enabled } else { enabled },
            keyboard,
            cx,
        ),
        SideRow::Context { line_no, text } => base
            .id(SharedString::from(format!("context-{row_index}")))
            .border_color(gpui::transparent_black())
            .child(div().w(theme::scaled_px(24.)).flex_shrink_0())
            .child(line_number(*line_no))
            .child(code_text(
                text.clone(),
                highlights,
                theme().text_main,
                &[],
                gpui::transparent_black(),
            )),
        SideRow::Blank { hunk_index } => base.id(SharedString::from(format!(
            "blank-{hunk_index}-{row_index}"
        ))),
        SideRow::Line {
            hunk_index,
            line_index,
            line_no,
            text,
            taken,
            diff_spans,
        } => {
            let (hunk, line) = (*hunk_index, *line_index);
            let taken = *taken;
            let id = format!("resolver-hunk-{hunk}-{}-line-{line}", side_key(side));
            let name = Msg::conflict_line(keyboard.borrow().side_name(side), *line_no);
            #[cfg(feature = "gui-e2e")]
            crate::ui::conflict_editor_a11y::record_checkbox(&id, &name, taken);
            let down_path = path.clone();
            let checkbox_path = path.clone();
            let move_path = path.clone();
            let key_path = path.clone();
            // The row owns focus. Remount the indicator on state changes rather
            // than retaining the pinned Checkbox's detached 250ms animation
            // after a virtual row (or its window) has been retired.
            let checkbox = gpui_component::checkbox::Checkbox::new(SharedString::from(format!(
                "{id}-checkbox-{taken}"
            )))
            .xsmall()
            .tab_stop(false)
            .disabled(!enabled)
            .checked(taken)
            .aria_label(SharedString::from(name.clone()))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    if enabled && view.editing.as_deref() == Some(checkbox_path.as_path()) {
                        view.resolver_keyboard
                            .borrow()
                            .focus_line(hunk, side, line, window, cx);
                        view.line_paint = Some((side, !taken));
                        view.conflict_editor_set_hunk_line(
                            &checkbox_path,
                            hunk,
                            side,
                            line,
                            !taken,
                        );
                        cx.notify();
                    }
                }),
            );
            let content = base
                .id(SharedString::from(id.clone()))
                .role(gpui::Role::CheckBox)
                .aria_label(SharedString::from(name))
                .aria_toggled(if taken {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .child(
                    div()
                        .w(theme::scaled_px(24.))
                        .flex_shrink_0()
                        .flex()
                        .justify_center()
                        .child(checkbox),
                )
                .child(line_number(*line_no))
                .child(code_text(
                    text.clone(),
                    if resolved && state == TriState::None {
                        Vec::new()
                    } else {
                        highlights
                    },
                    if resolved && state == TriState::None {
                        theme().text_sub
                    } else {
                        theme().text_main
                    },
                    if resolved && state == TriState::None {
                        &[]
                    } else {
                        diff_spans
                    },
                    gpui::transparent_black(),
                ))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |view, _, window, cx| {
                        if enabled && view.editing.as_deref() == Some(down_path.as_path()) {
                            view.resolver_keyboard
                                .borrow()
                                .focus_line(hunk, side, line, window, cx);
                            view.conflict_editor_set_hunk_line(
                                &down_path, hunk, side, line, !taken,
                            );
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |view, event: &gpui::MouseMoveEvent, _, cx| {
                        if enabled
                            && event.pressed_button == Some(gpui::MouseButton::Left)
                            && view.editing.as_deref() == Some(move_path.as_path())
                        {
                            if let Some((pane, target)) = view.line_paint {
                                if pane == side && target != taken {
                                    view.conflict_editor_set_hunk_line(
                                        &move_path, hunk, side, line, target,
                                    );
                                    cx.notify();
                                }
                            }
                        }
                    }),
                )
                .on_click(cx.listener(move |view, event: &gpui::ClickEvent, _, cx| {
                    if enabled && event.is_keyboard() {
                        view.conflict_editor_set_hunk_line(&key_path, hunk, side, line, !taken);
                        cx.notify();
                    }
                }))
                .relative()
                .child(e2e::measure_inside(id));
            crate::ui::conflict_editor_keys::line_navigation(
                content,
                hunk,
                side,
                line,
                taken,
                enabled,
                keyboard,
                &cx.entity(),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_action_row(
    base: gpui::Div,
    hunk: usize,
    path: &Arc<std::path::PathBuf>,
    side: SelectionSide,
    resolved: bool,
    state: TriState,
    background: u32,
    enabled: bool,
    keyboard: &std::rc::Rc<std::cell::RefCell<crate::ui::conflict_editor_keys::ResolverKeyboard>>,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    use kagi_git::resolution::HunkChoice;
    let key = side_key(side);
    let mut row = base
        .id(SharedString::from(format!(
            "conflict-hunk-{hunk}-{key}-actions"
        )))
        .role(gpui::Role::Toolbar)
        .aria_label(SharedString::from(format!("Conflict {}", hunk + 1)))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(1.))
                .bg(rgb(theme().elevated_border(theme().surface))),
        )
        .pl(theme::scaled_px(41.))
        .pr(theme::scaled_px(6.))
        .gap(theme::scaled_px(8.))
        .text_size(theme::scaled_px(11.5))
        .whitespace_nowrap()
        .child(
            div()
                .text_color(rgb(theme().text_sub))
                .child(format!("Conflict {}", hunk + 1)),
        );
    if resolved {
        let (message, color) = match state {
            TriState::All => (Msg::ConflictUsed, theme().color_success),
            TriState::None => (Msg::ConflictNotUsed, theme().color_blocker),
            TriState::Partial => (Msg::ConflictPartlyUsed, theme().text_main),
        };
        let id = format!("conflict-hunk-{hunk}-{key}-state");
        #[cfg(feature = "gui-e2e")]
        crate::ui::conflict_editor_a11y::record_status(&id, message.t());
        row = row.child(
            div()
                .id(SharedString::from(id.clone()))
                .role(gpui::Role::Status)
                .aria_label(message.t())
                .text_color(rgb(color))
                .when(state != TriState::Partial, |el| {
                    el.font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .child(message.t())
                .relative()
                .child(e2e::measure_inside(id)),
        );
    }
    let choices = if resolved {
        [
            Some((
                "change",
                Msg::ConflictChange,
                Msg::ConflictChangeAccessible,
                None,
                ButtonRole::NeutralTinted,
            )),
            None,
        ]
    } else {
        let (accept, both, label) = match side {
            SelectionSide::Current => (
                HunkChoice::AcceptCurrent,
                HunkChoice::BothCurrentFirst,
                Msg::ConflictBothCurrentFirst,
            ),
            SelectionSide::Incoming => (
                HunkChoice::AcceptIncoming,
                HunkChoice::BothIncomingFirst,
                Msg::ConflictBothIncomingFirst,
            ),
        };
        [
            Some((
                "use",
                Msg::ConflictUseThisSide,
                Msg::ConflictUseThisSideAccessible,
                Some(accept),
                ButtonRole::NeutralTinted,
            )),
            Some(("both", label, label, Some(both), ButtonRole::NeutralTinted)),
        ]
    };
    for (action, label, name, choice, role) in choices.into_iter().flatten() {
        let id = format!("conflict-hunk-{hunk}-{key}-{action}");
        let path = path.clone();
        let focus_id = id.clone();
        let button = crate::ui::conflict_editor_keys::adoption(
            id,
            name.t().to_owned(),
            state,
            label.t(),
            role,
            background,
            Some(hunk),
            enabled,
            keyboard,
            cx.listener(move |view, _, window, cx| {
                if view.editing.as_deref() != Some(path.as_path()) {
                    return;
                }
                view.conflict_editor_select_hunk(hunk);
                view.resolver_keyboard
                    .borrow()
                    .focus_control(hunk, &focus_id, window, cx);
                if let Some(choice) = &choice {
                    view.conflict_choose_hunk(&path, hunk, choice.clone(), cx);
                } else {
                    view.conflict_editor_reset_hunk(&path, hunk);
                }
                cx.notify();
            }),
            cx,
        );
        row = row.child(button);
    }
    row
}
