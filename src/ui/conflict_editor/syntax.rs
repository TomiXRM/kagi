use super::*;

// ────────────────────────────────────────────────────────────
// A/B row lists: file/chunk/line tri-state checkbox hierarchy (ADR-0071)
// ────────────────────────────────────────────────────────────

/// Per-row syntax-highlight spans for one side's row list, aligned by row
/// index to the `Vec<SideRow>` build order (ADR-0135).
pub type RowHl = Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>;

/// Cache for both sides' highlights. Row text is fixed for a given file within
/// a conflict session (selection only flips `taken` flags), so the key is the
/// path plus the theme key (a theme switch or reload re-colours the spans).
pub struct SideHlCache {
    key: (std::path::PathBuf, theme::ThemeKey),
    current: Arc<Vec<RowHl>>,
    incoming: Arc<Vec<RowHl>>,
}

/// Fetch (or lazily build) the highlight spans for `side`. Returns an empty
/// per-row vec when the language is unknown — rows then render as plain text.
pub(super) fn side_highlights(
    chrome: &EditorChrome,
    path: &std::path::Path,
    model: &kagi_git::resolution::HunkModel,
    side: SelectionSide,
) -> Arc<Vec<RowHl>> {
    let key = (path.to_path_buf(), theme().key());
    {
        let cache = chrome.hl_cache.borrow();
        if let Some(c) = cache.as_ref() {
            if c.key == key {
                return match side {
                    SelectionSide::Current => c.current.clone(),
                    SelectionSide::Incoming => c.incoming.clone(),
                };
            }
        }
    }
    let current = Arc::new(highlight_side_rows(
        &build_side_rows(model, SelectionSide::Current),
        path,
    ));
    let incoming = Arc::new(highlight_side_rows(
        &build_side_rows(model, SelectionSide::Incoming),
        path,
    ));
    let out = match side {
        SelectionSide::Current => current.clone(),
        SelectionSide::Incoming => incoming.clone(),
    };
    *chrome.hl_cache.borrow_mut() = Some(SideHlCache {
        key,
        current,
        incoming,
    });
    out
}

/// Tree-sitter highlight for a side's rows, mirroring `diff_view`'s proven
/// combine → parse once → distribute-spans-per-row approach (no sigil offset
/// here: ranges are row-local from byte 0).
fn highlight_side_rows(rows: &[SideRow], path: &std::path::Path) -> Vec<RowHl> {
    let texts: Vec<(usize, &str)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| match row {
            SideRow::Line { text, .. } => Some((i, text.as_str())),
            SideRow::Context { text, .. } => Some((i, text.as_str())),
            _ => None,
        })
        .collect();
    highlight_rows(rows.len(), &texts, path)
}

/// Tree-sitter highlight for `row_count` rows, of which `texts` names the ones
/// that carry code (row index → line text). Shared by the A/B panes and the
/// Result preview.
pub(super) fn highlight_rows(
    row_count: usize,
    texts: &[(usize, &str)],
    path: &std::path::Path,
) -> Vec<RowHl> {
    use gpui_component::highlighter::SyntaxHighlighter;
    use gpui_component::Rope;

    let mut out: Vec<RowHl> = vec![Vec::new(); row_count];
    let Some(lang) = crate::ui::diff_view::lang_for_path(path) else {
        return out;
    };

    let mut line_offsets: Vec<(usize, usize)> = Vec::new(); // (row_index, byte_start)
    let mut combined = String::new();
    for (i, text) in texts {
        line_offsets.push((*i, combined.len()));
        combined.push_str(text);
        combined.push('\n');
    }
    if combined.is_empty() {
        return out;
    }

    let mut highlighter = SyntaxHighlighter::new(lang);
    let rope = Rope::from_str(&combined);
    highlighter.update(None, &rope, None);
    let hl_theme = theme::highlight_theme(&theme::theme());
    let mut all_styles = highlighter.styles(&(0..combined.len()), &hl_theme);

    // Shared with the diff pane — see `distribute_highlights` for why the
    // obvious nested loop is not usable here. No sigil on these rows, so the
    // local offset is 0.
    crate::ui::diff_view::distribute_highlights(
        &mut all_styles,
        &line_offsets,
        combined.len(),
        0,
        |row_i, row_highlights| out[row_i] = row_highlights,
    );
    out
}
/// Monospace code text with validated syntax-highlight spans (the same
/// out-of-bounds guard as the diff renderers).
pub(super) fn code_text(
    text_value: String,
    highlights: RowHl,
    base_color: u32,
    diff_spans: &[Range<usize>],
    emph_bg: gpui::Hsla,
) -> gpui::Div {
    let shared = SharedString::from(text_value);
    let el = div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(theme::scaled_px(12.))
        .line_height(theme::scaled_px(18.))
        .font_family(terminal::pick_font_family())
        .text_color(rgb(base_color));
    if highlights.is_empty() && diff_spans.is_empty() {
        return el.child(shared);
    }
    let text_str: &str = shared.as_ref();
    let text_len = text_str.len();
    let valid: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = highlights
        .into_iter()
        .filter(|(r, _)| {
            r.start <= r.end
                && r.end <= text_len
                && text_str.is_char_boundary(r.start)
                && text_str.is_char_boundary(r.end)
        })
        .collect();
    // The word-diff emphasis merges with the syntax runs exactly as in the diff
    // pane (same helper, same fill + opaque underline), so a conflict row marks
    // the changed words instead of only the whole side.
    let merged = crate::ui::diff_split::merge_highlights(text_len, &valid, diff_spans, emph_bg);
    el.child(gpui::StyledText::new(shared.clone()).with_highlights(merged))
}
