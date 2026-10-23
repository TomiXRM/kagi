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
        .line_height(theme::scaled_px(18.))
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
        .h(theme::scaled_px(18.))
        .border_l(theme::scaled_px(3.));
    match row {
        SideRow::Context { line_no, text } => base
            .id(SharedString::from(format!("context-{row_index}")))
            .border_color(gpui::transparent_black())
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
            let content = base
                .border_color(rgb(theme().color_blocker))
                .bg(rgb(theme().diff_removed_bg))
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
                );
            let (hunk, line) = (*hunk_index, *line_index);
            let path = path.clone();
            let take = !*taken;
            crate::ui::conflict_editor_keys::adoption(
                format!("resolver-hunk-{hunk}-{}-line-{line}", side_key(side)),
                Msg::conflict_line(keyboard.borrow().side_name(side), *line_no),
                crate::ui::conflict_editor_keys::ControlState::Adoption(if *taken {
                    TriState::All
                } else {
                    TriState::None
                }),
                content,
                Some(hunk),
                enabled,
                keyboard,
                cx.listener(move |view, _: &gpui::ClickEvent, _, cx| {
                    view.conflict_editor_select_hunk(hunk);
                    view.conflict_editor_set_hunk_line(&path, hunk, side, line, take);
                    cx.notify();
                }),
                cx,
            )
            .h(theme::scaled_px(18.))
            .min_w(relative(1.))
            .into_any_element()
        }
    }
}
