//! Whole-file source rows; line selection remains the fine-grained decision tool.
use super::*;

#[derive(Clone)]
pub(super) enum SideRow {
    Line {
        hunk_index: usize,
        line_index: usize,
        line_no: usize,
        text: String,
        taken: bool,
        dim: bool,
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
                let (lines, other, taken) = match side {
                    SelectionSide::Current => (
                        &hunk.current,
                        &hunk.incoming,
                        hunk.line_select.as_ref().map(|s| &s.current_taken),
                    ),
                    SelectionSide::Incoming => (
                        &hunk.incoming,
                        &hunk.current,
                        hunk.line_select.as_ref().map(|s| &s.incoming_taken),
                    ),
                };
                let other_state = hunk.side_state(match side {
                    SelectionSide::Current => SelectionSide::Incoming,
                    SelectionSide::Incoming => SelectionSide::Current,
                });
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
                        dim: !is_taken && other_state != TriState::None,
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
                            render_source_row(
                                i,
                                row,
                                highlights.get(i).cloned().unwrap_or_default(),
                                &path,
                                side,
                                enabled,
                                &view.resolver_keyboard,
                                cx,
                            )
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
        .w(theme::scaled_px(47.))
        .pl(theme::scaled_px(9.))
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
    keyboard: &std::rc::Rc<std::cell::RefCell<crate::ui::conflict_editor_keys::ResolverKeyboard>>,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    let base = div()
        .flex()
        .items_center()
        .min_w(relative(1.))
        .h(theme::scaled_px(24.))
        .border_l(theme::scaled_px(3.));
    match row {
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
            ))
            .into_any_element(),
        SideRow::Blank { hunk_index } => base
            .id(SharedString::from(format!(
                "blank-{hunk_index}-{row_index}"
            )))
            .border_color(rgb(theme().color_blocker))
            .bg(rgb(theme().diff_removed_bg))
            .into_any_element(),
        SideRow::Line {
            hunk_index,
            line_index,
            line_no,
            text,
            taken,
            dim,
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
                .border_color(rgb(theme().color_blocker))
                .bg(rgb(theme().diff_removed_bg))
                .child(
                    div()
                        .w(theme::scaled_px(24.))
                        .flex_shrink_0()
                        .flex()
                        .justify_center()
                        .child(checkbox),
                )
                .child(line_number(*line_no))
                .child(
                    code_text(
                        text.clone(),
                        highlights,
                        theme().text_main,
                        diff_spans,
                        gpui::transparent_black(),
                    )
                    .when(*dim, |el| el.opacity(0.45)),
                )
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
            .into_any_element()
        }
    }
}
