//! Direct Result editing and the read-only saved-file renderer share code geometry.
use super::*;

pub struct ResultHlCache {
    key: (std::path::PathBuf, theme::ThemeKey, String),
    rows: Arc<Vec<RowHl>>,
}

fn result_highlights(
    chrome: &EditorChrome,
    path: &std::path::Path,
    lines: &[kagi_git::resolution::ResolvedLine],
) -> Arc<Vec<RowHl>> {
    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let key = (path.to_path_buf(), theme().key(), text);
    if let Some(cache) = chrome
        .result_hl
        .borrow()
        .as_ref()
        .filter(|cache| cache.key == key)
    {
        return cache.rows.clone();
    }
    let texts: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| (i, line.text.as_str()))
        .collect();
    let rows = Arc::new(highlight_rows(lines.len(), &texts, path));
    *chrome.result_hl.borrow_mut() = Some(ResultHlCache {
        key,
        rows: rows.clone(),
    });
    rows
}

fn origin_color(origin: LineOrigin) -> Option<u32> {
    match origin {
        LineOrigin::Current => Some(theme().color_branch),
        LineOrigin::Incoming => Some(theme().color_remote),
        LineOrigin::Manual => Some(theme().color_warning),
        LineOrigin::Context => None,
    }
}

pub(in crate::ui) fn render_saved_preview(
    lines: Arc<Vec<kagi_git::resolution::ResolvedLine>>,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    let highlights = result_highlights(chrome, path, &lines);
    let scroll = chrome.result_scroll.clone();
    div()
        .id("conflict-result-rows")
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_x_scroll()
        .child(
            uniform_list(
                "conflict-result-lines",
                lines.len(),
                cx.processor(move |_, range: Range<usize>, _, _| {
                    range
                        .filter_map(|i| lines.get(i).map(|line| (i, line)))
                        .map(|(i, line)| {
                            div()
                                .id(SharedString::from(format!("result-line-{i}")))
                                .flex()
                                .items_center()
                                .min_w(relative(1.))
                                .h(theme::scaled_px(18.))
                                .border_l(theme::scaled_px(3.))
                                .border_color(
                                    origin_color(line.origin)
                                        .map(rgb)
                                        .map(Into::<gpui::Hsla>::into)
                                        .unwrap_or_else(gpui::transparent_black),
                                )
                                .child(
                                    div()
                                        .w(theme::scaled_px(47.))
                                        .pl(theme::scaled_px(9.))
                                        .flex_shrink_0()
                                        .text_size(theme::scaled_px(11.))
                                        .text_color(rgb(theme().text_sub))
                                        .font_family(terminal::pick_font_family())
                                        .child((i + 1).to_string()),
                                )
                                .child(code_text(
                                    line.text.clone(),
                                    highlights.get(i).cloned().unwrap_or_default(),
                                    theme().text_main,
                                    &[],
                                    gpui::transparent_black(),
                                ))
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
        .into_any_element()
}

pub(super) fn render_result_pane(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    let Some(inputs) = chrome.inputs.as_ref().filter(|inputs| inputs.path == path) else {
        return div().flex_1().into_any_element();
    };
    let total = mode.buffer.hunk_count(path);
    let decided = mode
        .buffer
        .hunk_model(path)
        .map(|model| {
            model
                .regions
                .iter()
                .filter(|region| matches!(region, Region::Hunk(hunk) if hunk.is_resolved()))
                .count()
        })
        .unwrap_or(0);
    let status = format!("{} · {decided}/{total}", Msg::ConflictDraft.t());
    let editor = inputs.result.clone();
    let stripe_editor = editor.clone();
    let lines = if chrome.manual_override {
        mode.buffer
            .resolved_text(path)
            .map(|text| kagi_git::resolution::text_to_lines(&text, LineOrigin::Manual))
            .unwrap_or_default()
    } else {
        mode.buffer
            .hunk_model(path)
            .map(|model| model.assemble())
            .unwrap_or_default()
    };
    let mut offset = 0;
    let origins: Vec<_> = lines
        .iter()
        .map(|line| {
            let start = offset;
            offset += line.text.len() + 1;
            (start, origin_color(line.origin))
        })
        .collect();
    let stripes = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let state = stripe_editor.read(cx);
            let height = state.line_height().unwrap_or(theme::scaled_px(18.));
            for (offset, color) in &origins {
                let Some(color) = color else {
                    continue;
                };
                let Some(line) = state.range_to_bounds(&(*offset..*offset)) else {
                    continue;
                };
                if line.origin.y >= bounds.origin.y && line.origin.y + height <= bounds.bottom() {
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(bounds.origin.x, line.origin.y),
                            gpui::size(theme::scaled_px(3.), height),
                        ),
                        rgb(*color),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full();
    div()
        .id("conflict-pane-result")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .h(theme::scaled_px(26.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(theme::scaled_px(12.))
                .gap(theme::scaled_px(8.))
                .bg(rgb(theme().panel))
                .border_y_1()
                .border_color(rgb(theme().surface))
                .child(
                    div()
                        .text_size(theme::scaled_px(11.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(Msg::EditorResultOutput.t()),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(11.))
                        .text_color(rgb(theme().text_sub))
                        .child(status),
                ),
        )
        .child(e2e::measure_control(
            "conflict-result-editor",
            div()
                .relative()
                .flex_1()
                .min_h(px(0.))
                .font_family(terminal::pick_font_family())
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|view, _, _, cx| {
                        if !view.writer_busy {
                            view.result_editing = true;
                            cx.notify();
                        }
                    }),
                )
                .child(
                    Input::new(&editor)
                        .context_menu(kagi_ui_core::input::context_menu(
                            &editor,
                            chrome.writer_busy,
                        ))
                        .aria_label(Msg::A11yConflictResult.t())
                        .disabled(chrome.writer_busy)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .h_full()
                        .text_size(theme::scaled_px(12.))
                        .line_height(theme::scaled_px(18.))
                        .px(theme::scaled_px(12.))
                        .py(px(0.)),
                )
                .child(stripes),
        ))
        .into_any_element()
}
