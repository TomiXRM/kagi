//! Diff view-models + syntax highlighting for the diff panel.
//!
//! Presentation types (`DiffRow`, `FileDiffView`, `MainDiffView`, `CompareView`,
//! `MainDiffSource`, `CompareTarget`) built from `kagi::git` diff data, plus the
//! gpui-component tree-sitter highlighter glue (`lang_for_ext`,
//! `highlight_diff_rows`). No `KagiApp` coupling — extracted from `ui/mod.rs` as
//! part of the S6 view decomposition (architecture §2.4). Re-exported by
//! `ui/mod.rs` via `pub use diff_view::*;` so existing `crate::ui::*` paths resolve.

use std::path::PathBuf;

use gpui::{div, prelude::*, rgb, SharedString};

use kagi_git::{CommitId, DiffLineKind, FileDiff, FileStatus};

use super::theme;

// FileDiffView — pre-rendered diff rows for the diff panel
// ──────────────────────────────────────────────────────────────

/// A single displayable row in the diff viewer.
#[derive(Clone)]
pub enum DiffRow {
    /// A hunk header line (`@@ -a,b +c,d @@`).
    HunkHeader(SharedString),
    /// A content line (context / added / removed).
    Line {
        kind: DiffLineKind,
        /// The line content as a displayable string (with leading sigil stripped).
        text: SharedString,
        /// Old-side line number (None for Added lines).
        old_lineno: Option<u32>,
        /// New-side line number (None for Removed lines).
        new_lineno: Option<u32>,
        /// T-UI-004: Pre-computed syntax highlight spans (byte ranges + styles).
        /// Empty when the file type is unknown or highlighting failed.
        highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>,
    },
    /// Placeholder shown for binary files.
    Binary,
}

/// Pre-computed state for the diff view panel.
#[derive(Clone)]
pub struct FileDiffView {
    /// Display name of the file (path component).
    pub file_name: SharedString,
    /// All displayable rows: hunk headers + content lines.
    pub rows: Vec<DiffRow>,
    /// Row index into the commit's changed-files list (reserved for future
    /// navigation: e.g. "previous / next file" buttons in the diff panel).
    #[allow(dead_code)]
    pub file_index: usize,
}

impl FileDiffView {
    /// Build a [`FileDiffView`] from a [`FileDiff`] result.
    pub fn from_file_diff(file_diff: &FileDiff, file_index: usize) -> Self {
        let path = file_diff.new_path.as_ref().or(file_diff.old_path.as_ref());
        let file_name = SharedString::from(
            path.map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );

        let mut rows: Vec<DiffRow> = Vec::new();

        if file_diff.is_binary {
            rows.push(DiffRow::Binary);
        } else {
            for hunk in &file_diff.hunks {
                // Build hunk header string.
                let (os, oc) = hunk.old_range;
                let (ns, nc) = hunk.new_range;
                let header = SharedString::from(format!("@@ -{},{} +{},{} @@", os, oc, ns, nc));
                rows.push(DiffRow::HunkHeader(header));

                for line in &hunk.lines {
                    // Strip the trailing newline for display (keep content clean).
                    let raw = line.content.trim_end_matches('\n').trim_end_matches('\r');
                    // Add leading sigil for clarity.
                    let text = match line.kind {
                        DiffLineKind::Added => SharedString::from(format!("+{}", raw)),
                        DiffLineKind::Removed => SharedString::from(format!("-{}", raw)),
                        DiffLineKind::Context => SharedString::from(format!(" {}", raw)),
                    };
                    rows.push(DiffRow::Line {
                        kind: line.kind.clone(),
                        text,
                        old_lineno: line.old_lineno,
                        new_lineno: line.new_lineno,
                        highlights: vec![],
                    });
                }
            }
        }

        FileDiffView {
            file_name,
            rows,
            file_index,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// T-UI-003: MainDiffView — full-width main pane diff state
// ──────────────────────────────────────────────────────────────

/// Where the diff was opened from (used for re-load and navigation).
#[derive(Clone)]
pub enum MainDiffSource {
    /// Opened from the commit detail panel (changed-files list).
    Commit { row_index: usize, file_index: usize },
    /// Opened from the compare changed-files list.
    Compare {
        base: CommitId,
        target: CompareTarget,
        file_index: usize,
    },
    /// Opened from the Commit Panel — unstaged file.
    Unstaged { path: PathBuf },
    /// Opened from the Commit Panel — staged file.
    Staged { path: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareTarget {
    Head,
    WorkingTree,
}

#[derive(Clone, Debug)]
pub struct CompareView {
    pub base: CommitId,
    pub target: CompareTarget,
    pub files: Vec<FileStatus>,
    pub title: SharedString,
}

/// State for the full-width main pane diff view (T-UI-003).
#[derive(Clone)]
pub struct MainDiffView {
    /// Display title: file path.
    pub title: SharedString,
    /// Stats string: "+N −M".
    pub stats: SharedString,
    /// All displayable rows (hunk headers + content lines).
    pub rows: Vec<DiffRow>,
    /// Where this diff was opened from (for re-load / back navigation).
    #[allow(dead_code)]
    pub source: MainDiffSource,
}

// ──────────────────────────────────────────────────────────────
// T-UI-004: Syntax highlighting for diff rows
// ──────────────────────────────────────────────────────────────

/// Map a file extension to a language name understood by `gpui_component`'s
/// `LanguageRegistry`.  Returns `None` for unknown extensions.
pub(crate) fn lang_for_ext(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" => Some("javascript"),
        "ts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "json" | "jsonc" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "md" | "mdx" => Some("markdown"),
        "sh" | "bash" => Some("bash"),
        "c" => Some("c"),
        "cpp" | "cc" | "cxx" => Some("cpp"),
        "h" | "hpp" => Some("cpp"),
        "css" | "scss" => Some("css"),
        "html" | "htm" => Some("html"),
        "go" => Some("go"),
        "java" => Some("java"),
        "rb" => Some("ruby"),
        "zig" => Some("zig"),
        "sql" => Some("sql"),
        "swift" => Some("swift"),
        _ => None,
    }
}

/// T-UI-004: Apply syntax highlighting to a slice of `DiffRow`s in-place.
///
/// The file path's extension is used to select the language. If the language
/// is unknown or highlighting fails, rows are left with empty highlight spans
/// (plain-colour fallback).  Never panics.
///
/// Returns the language name that was used (or "none").
pub(crate) fn highlight_diff_rows(
    rows: &mut Vec<DiffRow>,
    file_path: &std::path::Path,
) -> &'static str {
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
    use gpui_component::Rope;

    // Determine language from extension.
    let lang = file_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(lang_for_ext);

    let lang = match lang {
        Some(l) => l,
        None => return "none",
    };

    // Build the full source text for the "new" side of the diff by concatenating
    // all Line rows.  We use a one-pass approach:
    //   1. Collect (text_without_sigil, byte_start_in_rope) for each Line row.
    //   2. Feed the combined text to the highlighter.
    //   3. Distribute the resulting (byte_range, style) spans back to each row,
    //      offsetting by byte_start_in_rope.
    //
    // The sigil (+/-/ ) at position 0 of each `text` is kept in the display string
    // but excluded from the highlighted region — highlights start at byte 1.

    let mut line_offsets: Vec<(usize, usize)> = Vec::new(); // (row_index, rope_byte_start)
    let mut combined = String::new();

    for (i, row) in rows.iter().enumerate() {
        if let DiffRow::Line { text, .. } = row {
            let t = text.as_ref();
            let start = combined.len();
            // Skip the leading sigil ('+', '-', ' ') for parsing purposes.
            // The highlight byte ranges will be relative to `combined`, which
            // starts after the sigil.
            let content = if !t.is_empty() { &t[1..] } else { "" };
            combined.push_str(content);
            combined.push('\n');
            line_offsets.push((i, start));
        }
    }

    if combined.is_empty() {
        return lang;
    }

    // Build highlighter and parse the combined source.
    let mut highlighter = SyntaxHighlighter::new(lang);
    let rope = Rope::from_str(&combined);
    highlighter.update(None, &rope);

    // Use a syntax-highlight theme matching the active UI theme's brightness
    // (W9-THEME): dark themes → default_dark, light themes → default_light.
    let hl_theme = if theme::theme().dark {
        HighlightTheme::default_dark()
    } else {
        HighlightTheme::default_light()
    };
    let all_styles = highlighter.styles(&(0..combined.len()), &hl_theme);

    // Distribute styles back to rows.
    // For each row we know: rope_byte_start (start of content inside `combined`,
    // i.e. after the sigil) and rope_byte_end = start_of_next_row - 1 (the \n).
    for k in 0..line_offsets.len() {
        let (row_i, rope_start) = line_offsets[k];
        let rope_end = if k + 1 < line_offsets.len() {
            line_offsets[k + 1].1
        } else {
            combined.len()
        };
        // The content slice is rope_start..rope_end (excludes the trailing \n).
        let content_end = rope_end.saturating_sub(1); // strip the \n

        // Collect highlight spans that overlap [rope_start, content_end).
        let mut row_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        for (range, style) in &all_styles {
            let clipped_start = range.start.max(rope_start);
            let clipped_end = range.end.min(content_end);
            if clipped_start >= clipped_end {
                continue;
            }
            // Translate back to row-local byte offsets (offset by 1 for the sigil).
            let local_start = 1 + (clipped_start - rope_start);
            let local_end = 1 + (clipped_end - rope_start);
            row_highlights.push((local_start..local_end, *style));
        }

        if let DiffRow::Line { highlights, .. } = &mut rows[row_i] {
            *highlights = row_highlights;
        }
    }

    lang
}

/// Off-thread-friendly variant of [`highlight_diff_rows`] (ADR-0109 / perf).
///
/// Instead of mutating `rows` in place (which requires `&mut Vec<DiffRow>` and
/// is therefore `!Send` when the rows hold GPUI types), this returns a
/// `Send`-safe vector of `(row_index, highlights)` pairs that the caller can
/// move across a `cx.background_spawn` boundary and apply on the UI thread.
///
/// The parsing work (tree-sitter) is identical to `highlight_diff_rows`; only
/// the output shape differs. Callers that stay on the UI thread should keep
/// using `highlight_diff_rows` for simplicity; callers that want to render the
/// diff text immediately and swap highlights in when ready should use this.
pub(crate) fn highlight_diff_rows_send(
    rows: &[DiffRow],
    file_path: &std::path::Path,
) -> (
    String,
    Vec<(usize, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>)>,
) {
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
    use gpui_component::Rope;

    let lang = file_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(lang_for_ext)
        .unwrap_or("none");

    // Collect (row_index, rope_byte_start) for each Line row and build the
    // combined source text — same one-pass approach as highlight_diff_rows.
    let mut line_offsets: Vec<(usize, usize)> = Vec::new();
    let mut combined = String::new();
    for (i, row) in rows.iter().enumerate() {
        if let DiffRow::Line { text, .. } = row {
            let t = text.as_ref();
            let start = combined.len();
            let content = if !t.is_empty() { &t[1..] } else { "" };
            combined.push_str(content);
            combined.push('\n');
            line_offsets.push((i, start));
        }
    }

    if combined.is_empty() || lang == "none" {
        return (lang.to_string(), Vec::new());
    }

    let mut highlighter = SyntaxHighlighter::new(lang);
    let rope = Rope::from_str(&combined);
    highlighter.update(None, &rope);
    let hl_theme = if theme::theme().dark {
        HighlightTheme::default_dark()
    } else {
        HighlightTheme::default_light()
    };
    let all_styles = highlighter.styles(&(0..combined.len()), &hl_theme);

    // Distribute styles back to per-row vectors (Send-safe: no DiffRow, just
    // the row index + the highlight span list).
    let mut result: Vec<(usize, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>)> =
        Vec::with_capacity(line_offsets.len());
    for k in 0..line_offsets.len() {
        let (row_i, rope_start) = line_offsets[k];
        let rope_end = if k + 1 < line_offsets.len() {
            line_offsets[k + 1].1
        } else {
            combined.len()
        };
        let content_end = rope_end.saturating_sub(1);

        let mut row_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        for (range, style) in &all_styles {
            let clipped_start = range.start.max(rope_start);
            let clipped_end = range.end.min(content_end);
            if clipped_start >= clipped_end {
                continue;
            }
            let local_start = 1 + (clipped_start - rope_start);
            let local_end = 1 + (clipped_end - rope_start);
            row_highlights.push((local_start..local_end, *style));
        }
        result.push((row_i, row_highlights));
    }

    (lang.to_string(), result)
}

/// Render a range of diff rows for the `"main-diff-list"` uniform_list.
/// Includes line numbers: old/new each 5 chars wide, theme::theme().text_muted colour.
pub(crate) fn render_main_diff_rows(
    rows: &[DiffRow],
    range: std::ops::Range<usize>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(i, row)| match row {
            DiffRow::HunkHeader(header) => div()
                .id(("main-diff-hunk", i))
                .w_full()
                .px_2()
                .py_px()
                .bg(rgb(theme::theme().surface))
                .text_sm()
                .text_color(rgb(theme::theme().diff_hunk))
                .overflow_hidden()
                .child(header.clone())
                .into_any(),
            DiffRow::Line {
                kind,
                text,
                old_lineno,
                new_lineno,
                highlights,
            } => {
                let bg = match kind {
                    DiffLineKind::Added => theme::theme().diff_added_bg,
                    DiffLineKind::Removed => theme::theme().diff_removed_bg,
                    DiffLineKind::Context => theme::theme().bg_base,
                };
                // Theme tokens (not hardcoded hex): light themes tune these to
                // dark green/red so the text stays readable on the light diff
                // backgrounds — the old fixed light-green/red washed out there.
                let text_color = match kind {
                    DiffLineKind::Added => theme::theme().change_added,
                    DiffLineKind::Removed => theme::theme().change_deleted,
                    DiffLineKind::Context => theme::theme().text_main,
                };
                // Format line numbers: 5 chars fixed width, muted colour.
                let old_str = match old_lineno {
                    Some(n) => format!("{:5}", n),
                    None => "     ".to_string(),
                };
                let new_str = match new_lineno {
                    Some(n) => format!("{:5}", n),
                    None => "     ".to_string(),
                };

                // T-UI-004: build highlighted content element.
                // If we have pre-computed highlight spans, use StyledText; otherwise
                // fall back to a plain text element (keeps the existing colour).
                let content_el: gpui::AnyElement = if highlights.is_empty() {
                    div()
                        .flex_1()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .child(text.clone())
                        .into_any()
                } else {
                    // Validate that all highlight byte ranges lie within the text.
                    // Silently drop spans that fall outside to prevent panics.
                    let text_str: &str = text.as_ref();
                    let text_len = text_str.len();
                    let valid_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> =
                        highlights
                            .iter()
                            .filter(|(r, _)| {
                                r.start <= r.end
                                    && r.end <= text_len
                                    && text_str.is_char_boundary(r.start)
                                    && text_str.is_char_boundary(r.end)
                            })
                            .cloned()
                            .collect();
                    div()
                        .flex_1()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .child(
                            gpui::StyledText::new(text.clone()).with_highlights(valid_highlights),
                        )
                        .into_any()
                };

                div()
                    .id(("main-diff-line", i))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .py_px()
                    .bg(rgb(bg))
                    .text_sm()
                    .overflow_hidden()
                    // Old line number
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(theme::scaled_px(44.))
                            .text_color(rgb(theme::theme().text_muted))
                            .child(SharedString::from(old_str)),
                    )
                    // New line number
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(theme::scaled_px(44.))
                            .text_color(rgb(theme::theme().text_muted))
                            .child(SharedString::from(new_str)),
                    )
                    // Content (sigil + highlighted text)
                    .child(content_el)
                    .into_any()
            }
            DiffRow::Binary => div()
                .id(("main-diff-binary", i))
                .w_full()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(rgb(theme::theme().text_muted))
                .child(SharedString::from("Binary file (no diff)"))
                .into_any(),
        })
        .collect()
}
