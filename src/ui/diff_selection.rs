//! Line-range selection + copy for the diff renderers (R1, requirements
//! session 2026-08-12).
//!
//! Drag over diff rows to select a line range, ⌘C to copy the CODE text —
//! no +/- sigils, no line numbers — because the agreed destinations are AI
//! chats, editors and grep, all of which want paste-able code.
//!
//! One process-global selection (the `diff_split` flag idiom): the row
//! renderers are free functions shared by the main pane, File History and the
//! Editor Workspace, so entity-owned state would need three parallel plumbing
//! paths for a strictly single-selection interaction. The selection carries a
//! `key` (hash of the diff title) so only the surface it was made on paints
//! highlights, and it stores the extracted TEXT eagerly at mouse-up, so ⌘C
//! needs no access to any pane's rows.

use std::sync::Mutex;

use super::diff_view::DiffRow;

struct Selection {
    key: u64,
    anchor: usize,
    cursor: usize,
    /// Extracted at mouse-up (and single click) — what ⌘C puts on the clipboard.
    text: String,
}

static SELECTION: Mutex<Option<Selection>> = Mutex::new(None);

/// Identity for one diff surface. Title + row count is stable across frames
/// and distinct enough between the visible panes; a collision only paints a
/// spurious highlight, never copies the wrong text (text is stored, not read).
pub(crate) fn surface_key(title: &str, row_count: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (title, row_count).hash(&mut h);
    h.finish()
}

/// Mouse-down on row `ix`: start a new selection (single row until dragged).
pub(crate) fn begin(key: u64, ix: usize) {
    *SELECTION.lock().unwrap() = Some(Selection {
        key,
        anchor: ix,
        cursor: ix,
        text: String::new(),
    });
}

/// Drag over row `ix`: extend. Returns true when the range changed (repaint).
pub(crate) fn drag_to(key: u64, ix: usize) -> bool {
    let mut sel = SELECTION.lock().unwrap();
    match sel.as_mut() {
        Some(s) if s.key == key && s.cursor != ix => {
            s.cursor = ix;
            true
        }
        _ => false,
    }
}

/// The inclusive selected range for surface `key`, if the selection is on it.
pub(crate) fn range(key: u64) -> Option<(usize, usize)> {
    let sel = SELECTION.lock().unwrap();
    sel.as_ref()
        .filter(|s| s.key == key)
        .map(|s| (s.anchor.min(s.cursor), s.anchor.max(s.cursor)))
}

/// Whether row `ix` on surface `key` is inside the selection (highlight test).
pub(crate) fn contains(key: u64, ix: usize) -> bool {
    range(key).is_some_and(|(lo, hi)| ix >= lo && ix <= hi)
}

/// Store the copyable text (called at mouse-up with the freshly built text).
pub(crate) fn set_text(key: u64, text: String) {
    let mut sel = SELECTION.lock().unwrap();
    if let Some(s) = sel.as_mut() {
        if s.key == key {
            s.text = text;
        }
    }
}

/// The stored text, if any selection exists (⌘C reads this).
pub(crate) fn selected_text() -> Option<String> {
    let sel = SELECTION.lock().unwrap();
    sel.as_ref()
        .map(|s| s.text.clone())
        .filter(|t| !t.is_empty())
}

/// Clear the selection. Returns whether one existed (Esc-chain short-circuit).
pub(crate) fn clear() -> bool {
    SELECTION.lock().unwrap().take().is_some()
}

/// Build the clipboard text for rows `[lo, hi]`: content lines only, sigil
/// stripped, hunk headers and binary rows skipped.
pub(crate) fn build_text(rows: &[DiffRow], lo: usize, hi: usize) -> String {
    let mut out = String::new();
    for row in rows.iter().take(hi + 1).skip(lo) {
        if let DiffRow::Line { text, .. } = row {
            let t: &str = text.as_ref();
            // The display text carries the diff sigil ('+' / '-' / ' ') at
            // byte 0; the copy destinations want plain code.
            let content = t.get(1..).unwrap_or("");
            out.push_str(content);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::diff::DiffLineKind;

    fn line(text: &str) -> DiffRow {
        DiffRow::Line {
            kind: DiffLineKind::Added,
            text: gpui::SharedString::from(text.to_string()),
            old_lineno: None,
            new_lineno: Some(1),
            highlights: Vec::new(),
        }
    }

    #[test]
    fn copy_strips_sigils_and_skips_hunk_headers() {
        let rows = vec![
            DiffRow::HunkHeader(gpui::SharedString::from("@@ -1 +1 @@")),
            line("+let a = 1;"),
            line("-let a = 2;"),
            line(" let b = 3;"),
        ];
        assert_eq!(
            build_text(&rows, 0, 3),
            "let a = 1;\nlet a = 2;\nlet b = 3;\n"
        );
    }

    #[test]
    fn selection_lifecycle_and_key_isolation() {
        clear();
        let (k1, k2) = (surface_key("a.rs", 10), surface_key("b.rs", 10));
        begin(k1, 2);
        assert!(drag_to(k1, 5));
        assert!(!drag_to(k2, 7), "another surface must not steal the drag");
        assert_eq!(range(k1), Some((2, 5)));
        assert_eq!(range(k2), None);
        assert!(contains(k1, 3) && !contains(k1, 6));
        set_text(k1, "x\n".into());
        assert_eq!(selected_text().as_deref(), Some("x\n"));
        assert!(clear());
        assert!(selected_text().is_none());
    }
}
