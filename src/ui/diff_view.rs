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
use super::{commit_panel, diff_view, KagiApp};
use gpui::Context;

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

// Main-diff open/step methods on `KagiApp`, moved from `src/ui/mod.rs`
// (T-HOTSPOT-UIMOD-001). Behaviour-preserving relocation.
impl KagiApp {
    /// T-UI-003: Open the diff for the file at `file_index` in the currently
    /// selected commit in the full-width main pane.
    ///
    /// Emits the legacy `[kagi] diff:` log (headless compat) plus
    /// `[kagi] main-diff: open <path> rows=N`.
    /// No-op if no commit is selected.
    /// Step the open main diff to the previous/next file (arrow keys).
    /// No-op when no diff is open or already at the list edge.
    pub fn main_diff_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let source = match self.main_diff.as_ref() {
            Some(d) => d.source.clone(),
            None => return,
        };
        match source {
            MainDiffSource::Commit {
                row_index,
                file_index,
            } => {
                let len = self
                    .diff_caches
                    .changed_files
                    .get(&row_index)
                    .and_then(|o| o.as_ref())
                    .map(|v| v.len())
                    .unwrap_or(0);
                if len == 0 {
                    return;
                }
                let next = (file_index as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != file_index {
                    self.open_main_diff_commit(next, cx);
                }
            }
            MainDiffSource::Compare {
                base,
                target,
                file_index,
            } => {
                let len = match self.compare_view.as_ref() {
                    Some(view) if view.base == base && view.target == target => view.files.len(),
                    _ => 0,
                };
                if len == 0 {
                    return;
                }
                let next = (file_index as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != file_index {
                    self.open_main_diff_compare(next);
                }
            }
            MainDiffSource::Unstaged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(e) => {
                        let p = &e.read(cx).state;
                        (
                            p.unstaged.iter().position(|f| f.path == path),
                            p.unstaged.len(),
                        )
                    }
                    None => return,
                };
                let cur = match cur {
                    Some(c) => c,
                    None => return,
                };
                if len == 0 {
                    return;
                }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(
                        commit_panel::CommitPanelFileRef::Unstaged { index: next },
                        cx,
                    );
                }
            }
            MainDiffSource::Staged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(e) => {
                        let p = &e.read(cx).state;
                        (p.staged.iter().position(|f| f.path == path), p.staged.len())
                    }
                    None => return,
                };
                let cur = match cur {
                    Some(c) => c,
                    None => return,
                };
                if len == 0 {
                    return;
                }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(
                        commit_panel::CommitPanelFileRef::Staged { index: next },
                        cx,
                    );
                }
            }
        }
    }

    /// Build the full-width [`MainDiffView`] for a commit's file diff. Shared by
    /// the local (`git2`) path and the remote (SSH) path so both render
    /// identically.
    pub(crate) fn set_commit_main_diff(
        &mut self,
        file_diff: &FileDiff,
        path: &std::path::Path,
        selected: usize,
        file_index: usize,
        cx: Option<&mut Context<Self>>,
    ) {
        let added: usize = file_diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == DiffLineKind::Added)
            .count();
        let removed: usize = file_diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == DiffLineKind::Removed)
            .count();
        eprintln!(
            "[kagi] diff: {} hunks={} (+{} -{})",
            path.display(),
            file_diff.hunks.len(),
            added,
            removed,
        );

        let fdv = FileDiffView::from_file_diff(file_diff, file_index);
        let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
        let title = fdv.file_name.clone();

        match cx {
            // UI path (cx present): render text-first, highlight off-thread.
            Some(cx) => {
                let rows = fdv.rows;
                let row_count = rows.len();
                let path_for_hl = path.to_path_buf();
                let rows_snapshot = rows.clone();
                let selected_for_hl = selected;
                let file_index_for_hl = file_index;

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Commit {
                        row_index: selected,
                        file_index,
                    },
                });

                // Spawn the highlight off-thread; store the result for swap-in.
                cx.spawn(async move |this, acx| {
                    let (hl_lang, highlights) =
                        diff_view::highlight_diff_rows_send(&rows_snapshot, &path_for_hl);
                    let _ = this.update(acx, |app, cx| {
                        app.pending_diff_highlight =
                            Some((selected_for_hl, file_index_for_hl, highlights));
                        eprintln!(
                            "[kagi] main-diff: highlight ready {} rows={} lang={}",
                            path_for_hl.display(),
                            row_count,
                            hl_lang
                        );
                        cx.notify();
                    });
                })
                .detach();
            }
            // Headless path (no cx): synchronous highlight (test-only, no UI).
            None => {
                let mut rows = fdv.rows;
                let hl_lang = diff_view::highlight_diff_rows(&mut rows, path);
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    rows.len(),
                    hl_lang
                );
                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Commit {
                        row_index: selected,
                        file_index,
                    },
                });
            }
        }
    }

    pub fn open_main_diff_compare(&mut self, file_index: usize) {
        let _repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let view = match self.compare_view.as_ref() {
            Some(v) => v.clone(),
            None => return,
        };
        let file_status = match view.files.get(file_index) {
            Some(f) => f,
            None => return,
        };
        let path = file_status.path.clone();

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

        let file_diff_result = match view.target {
            CompareTarget::Head => {
                let head = match repo.head_commit_id() {
                    Some(id) => id,
                    None => return,
                };
                repo.compare_file_diff(&view.base, &head, &path)
            }
            CompareTarget::WorkingTree => {
                repo.compare_commit_to_workdir_file_diff(&view.base, &path)
            }
        };

        match file_diff_result {
            Ok(file_diff) => {
                let added: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                let hunks = file_diff.hunks.len();

                eprintln!(
                    "[kagi] diff: {} hunks={} (+{} -{})",
                    path.display(),
                    hunks,
                    added,
                    removed,
                );

                let fdv = FileDiffView::from_file_diff(&file_diff, file_index);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    row_count,
                    hl_lang
                );

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Compare {
                        base: view.base,
                        target: view.target,
                        file_index,
                    },
                });
            }
            Err(e) => {
                klog!("compare diff error: {}", e);
            }
        }
    }

    /// T-UI-003: Open the diff for a Commit Panel file in the full-width main pane.
    pub fn open_main_diff_wip(
        &mut self,
        file_ref: commit_panel::CommitPanelFileRef,
        cx: &mut Context<Self>,
    ) {
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let entity = match self.commit_panel.as_ref() {
            Some(e) => e.clone(),
            None => return,
        };

        let (is_staged, path) = {
            let panel = &entity.read(cx).state;
            match &file_ref {
                commit_panel::CommitPanelFileRef::Unstaged { index } => {
                    if let Some(f) = panel.unstaged.get(*index) {
                        (false, f.path.clone())
                    } else {
                        return;
                    }
                }
                commit_panel::CommitPanelFileRef::Staged { index } => {
                    if let Some(f) = panel.staged.get(*index) {
                        (true, f.path.clone())
                    } else {
                        return;
                    }
                }
            }
        };

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

        let file_diff_result = if is_staged {
            repo.staged_file_diff(&path)
        } else {
            repo.unstaged_file_diff(&path)
        };

        match file_diff_result {
            Ok(fd) => {
                let added: usize = fd
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = fd
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                eprintln!(
                    "[kagi] commit-panel diff: {} (+{} -{})",
                    path.display(),
                    added,
                    removed
                );

                let fdv = FileDiffView::from_file_diff(&fd, 0);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                // T-UI-004: apply syntax highlighting once at open time.
                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    row_count,
                    hl_lang
                );

                let source = if is_staged {
                    MainDiffSource::Staged { path }
                } else {
                    MainDiffSource::Unstaged { path }
                };
                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source,
                });
            }
            Err(e) => {
                klog!("commit-panel diff error: {}", e);
            }
        }
    }
}

// Remaining main-diff open/close/highlight methods on `KagiApp`, moved from
// `src/ui/mod.rs` (T-HOTSPOT-UIMOD-001). Behaviour-preserving relocation.
impl KagiApp {
    pub fn open_main_diff_inspector_file(&mut self, file_index: usize, cx: &mut Context<Self>) {
        if self.remote_view.is_some() {
            // Remote read-only view (ADR-0089 Phase 2c): the file diff is an SSH
            // round-trip, loaded off-thread.
            self.open_remote_main_diff(file_index, cx);
        } else if self.compare_view.is_some() {
            self.open_main_diff_compare(file_index);
        } else {
            self.open_main_diff_commit(file_index, cx);
        }
    }

    /// Open the first changed file's diff in the main pane (headless path).
    /// Calls the synchronous highlight variant since headless has no cx and is
    /// test-only (no UI latency concern).
    pub fn open_main_diff_commit_headless(&mut self, file_index: usize) {
        // Delegate to the shared open path with a dummy sync-highlight by
        // calling set_commit_main_diff_sync directly after acquiring the diff.
        self.open_main_diff_commit_inner(file_index, None);
    }

    /// Open the main diff with async highlight (UI path).
    pub fn open_main_diff_commit(&mut self, file_index: usize, cx: &mut Context<Self>) {
        self.open_main_diff_commit_inner(file_index, Some(cx));
    }

    fn open_main_diff_commit_inner(
        &mut self,
        file_index: usize,
        mut cx: Option<&mut Context<Self>>,
    ) {
        use kagi_git::CommitId;

        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let _repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let detail = match self.active_view.details.get(selected) {
            Some(d) => d,
            None => return,
        };
        let files = match self
            .diff_caches
            .changed_files
            .get(&selected)
            .and_then(|v| v.as_ref())
        {
            Some(f) => f,
            None => return,
        };
        let file_status = match files.get(file_index) {
            Some(f) => f,
            None => return,
        };

        let id = CommitId(detail.full_sha.as_ref().to_string());
        let path = file_status.path.clone();

        // T-REARCH-031: per-(row, file) content cache. Clicking between two
        // commits to compare the same file previously recomputed the full git2
        // tree-diff + hunk extraction on every toggle. Hit the cache first.
        if let Some(cached) = self
            .diff_caches
            .file_content
            .get(&(selected, file_index))
            .cloned()
        {
            self.set_commit_main_diff(&cached, &path, selected, file_index, cx.as_deref_mut());
            return;
        }

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

        match repo.commit_file_diff(&id, &path) {
            Ok(file_diff) => {
                let arc = std::sync::Arc::new(file_diff);
                self.diff_caches
                    .file_content
                    .insert((selected, file_index), arc.clone());
                self.set_commit_main_diff(&arc, &path, selected, file_index, cx.as_deref_mut());
            }
            Err(e) => {
                klog!("diff error: {}", e);
            }
        }
    }

    /// Apply a pending background highlight result to `main_diff` if it still
    /// matches the current view (same row/file index). Called from render so
    /// the swap happens on the next frame after the background task completes.
    /// Stale results (view changed) are discarded.
    pub(crate) fn apply_pending_highlights(&mut self) {
        let Some((row, file, highlights)) = self.pending_diff_highlight.take() else {
            return;
        };
        let Some(view) = self.main_diff.as_mut() else {
            return;
        };
        // Only apply if the view hasn't changed since the highlight was requested.
        match view.source {
            MainDiffSource::Commit {
                row_index,
                file_index,
            } if row_index == row && file_index == file => {}
            _ => return,
        }
        for (row_i, row_highlights) in highlights {
            if let Some(DiffRow::Line { highlights: hl, .. }) = view.rows.get_mut(row_i) {
                *hl = row_highlights;
            }
        }
    }

    /// Open the full-width diff for a clicked file of the selected remote commit,
    /// loading the unified diff over SSH off the UI thread (ADR-0089 Phase 2c).
    fn open_remote_main_diff(&mut self, file_index: usize, cx: &mut Context<Self>) {
        let (host, root) = match &self.remote_view {
            Some(v) => (v.host.clone(), v.root.clone()),
            None => return,
        };
        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let sha = match self.active_view.details.get(selected) {
            Some(d) => d.full_sha.as_ref().to_string(),
            None => return,
        };
        let path = match self
            .diff_caches
            .changed_files
            .get(&selected)
            .and_then(|v| v.as_ref())
            .and_then(|files| files.get(file_index))
        {
            Some(f) => f.path.clone(),
            None => return,
        };
        let path_str = path.to_string_lossy().into_owned();

        let task = cx.background_spawn(async move {
            kagi::remote::remote_commit_file_diff(&host, &root, &sha, &path_str)
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                match result {
                    Ok(file_diff) => {
                        app.set_commit_main_diff(&file_diff, &path, selected, file_index, Some(cx))
                    }
                    Err(e) => klog!("remote diff error: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// T-UI-003: Close the main diff view and return to the commit graph.
    /// No-op when main_diff is None.
    pub fn close_main_diff(&mut self) {
        self.main_diff = None;
    }
}
