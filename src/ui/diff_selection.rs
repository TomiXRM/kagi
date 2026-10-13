//! Line-range selection + copy for the diff renderers (R1, requirements
//! session 2026-08-12).
//!
//! Drag over diff rows to select a line range, ⌘C to copy the CODE text —
//! no +/- sigils, no line numbers — because the agreed destinations are AI
//! chats, editors and grep, all of which want paste-able code.
//!
//! A single selection is shared by the free-function renderers, but belongs to
//! one session, retained list and content revision. Highlight-only row changes
//! keep the revision. Replacement and owner retirement invalidate it; only the
//! focused body may read its saved code for Copy.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::diff_view::DiffRow;

pub(crate) mod keyboard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionOwner {
    Main(crate::app::SessionId),
    History(crate::app::SessionId),
    Editor(crate::app::SessionId),
    Pr(crate::app::SessionId),
    #[cfg(any(test, feature = "gui-e2e"))]
    Test,
}

impl SelectionOwner {
    fn session(self) -> Option<crate::app::SessionId> {
        match self {
            Self::Main(owner) | Self::History(owner) | Self::Editor(owner) | Self::Pr(owner) => {
                Some(owner)
            }
            #[cfg(any(test, feature = "gui-e2e"))]
            Self::Test => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectionKey {
    owner: SelectionOwner,
    surface: kagi_ui_core::diff_list::DiffListSource,
    revision: kagi_ui_core::diff_list::DiffListSource,
}

impl SelectionKey {
    pub(crate) fn new(
        owner: SelectionOwner,
        surface: kagi_ui_core::diff_list::DiffListSource,
        revision: kagi_ui_core::diff_list::DiffListSource,
    ) -> Self {
        Self {
            owner,
            surface,
            revision,
        }
    }

    #[cfg(feature = "gui-e2e")]
    pub(crate) fn token(self) -> u64 {
        self.revision.token()
    }
}

struct Selection {
    key: SelectionKey,
    anchor: usize,
    cursor: usize,
    /// Headers show a caret only for keyboard navigation, not mouse selection.
    keyboard_caret: bool,
    /// Split navigation keeps its column affinity across filler/header rows.
    split_right: Option<bool>,
    /// Extracted at mouse-up (and single click) — what ⌘C puts on the clipboard.
    text: String,
}

static SELECTION: Mutex<Option<Selection>> = Mutex::new(None);

fn selection() -> MutexGuard<'static, Option<Selection>> {
    SELECTION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Mouse-down on row `ix`: start a new selection (single row until dragged).
pub(crate) fn begin(key: SelectionKey, ix: usize) {
    *selection() = Some(Selection {
        key,
        anchor: ix,
        cursor: ix,
        keyboard_caret: false,
        split_right: None,
        text: String::new(),
    });
}

/// Drag over row `ix`: extend. Returns true when the range changed (repaint).
pub(crate) fn drag_to(key: SelectionKey, ix: usize) -> bool {
    let mut sel = selection();
    match sel.as_mut() {
        Some(s) if s.key == key && s.cursor != ix => {
            s.cursor = ix;
            true
        }
        _ => false,
    }
}

/// The inclusive selected range for surface `key`, if the selection is on it.
pub(crate) fn range(key: SelectionKey) -> Option<(usize, usize)> {
    let sel = selection();
    sel.as_ref()
        .filter(|s| s.key == key)
        .map(|s| (s.anchor.min(s.cursor), s.anchor.max(s.cursor)))
}

/// Whether row `ix` on surface `key` is inside the selection (highlight test).
pub(crate) fn contains(key: SelectionKey, ix: usize) -> bool {
    range(key).is_some_and(|(lo, hi)| ix >= lo && ix <= hi)
}

pub(crate) fn is_keyboard_caret(key: SelectionKey, ix: usize) -> bool {
    selection()
        .as_ref()
        .is_some_and(|s| s.key == key && s.keyboard_caret && s.cursor == ix)
}

/// Store the copyable text (called at mouse-up with the freshly built text).
pub(crate) fn set_text(key: SelectionKey, text: String) {
    let mut sel = selection();
    if let Some(s) = sel.as_mut() {
        if s.key == key {
            s.text = text;
        }
    }
}

/// Saved code only when the focused surface still owns this exact revision.
pub(crate) fn selected_text(key: SelectionKey) -> Option<String> {
    let sel = selection();
    sel.as_ref()
        .filter(|s| s.key == key && !s.text.is_empty())
        .map(|s| s.text.clone())
}

/// Clear the selection. Returns whether one existed (Esc-chain short-circuit).
pub(crate) fn clear() -> bool {
    selection().take().is_some()
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn has_selection() -> bool {
    selection().is_some()
}

fn retire_if(predicate: impl FnOnce(SelectionKey) -> bool) -> bool {
    let mut sel = selection();
    if sel.as_ref().is_some_and(|s| predicate(s.key)) {
        sel.take();
        true
    } else {
        false
    }
}

pub(crate) fn retire_owner(owner: SelectionOwner) {
    retire_if(|key| key.owner == owner);
}

pub(crate) fn retire_session(owner: crate::app::SessionId) {
    retire_if(|key| key.owner.session() == Some(owner));
}

pub(crate) fn retire_surface(surface: kagi_ui_core::diff_list::DiffListSource) {
    retire_if(|key| key.surface == surface);
}

pub(crate) fn retire_revision(revision: kagi_ui_core::diff_list::DiffListSource) {
    retire_if(|key| key.revision == revision);
}

/// A retained list can replace its source without replacing its focus handle.
pub(crate) fn sync(key: SelectionKey) {
    retire_if(|previous| previous.surface == key.surface && previous != key);
}

/// Move the shared inclusive caret/range and eagerly refresh code-only copy text.
fn move_rows(
    key: SelectionKey,
    rows: &[DiffRow],
    step: i8,
    extend: bool,
    boundary: bool,
    all: bool,
) -> Option<usize> {
    let last = rows.len().checked_sub(1)?;
    let mut selection = selection();
    let current = selection.as_ref().filter(|s| s.key == key);
    let cursor = current.map_or(0, |s| s.cursor.min(last));
    let target = if all || (boundary && step > 0) {
        last
    } else if boundary {
        0
    } else if step > 0 {
        cursor.saturating_add(1).min(last)
    } else {
        cursor.saturating_sub(1)
    };
    let anchor = if all {
        0
    } else if extend {
        current.map_or(0, |s| s.anchor.min(last))
    } else {
        target
    };
    *selection = Some(Selection {
        key,
        split_right: None,
        anchor,
        cursor: target,
        keyboard_caret: true,
        text: build_text(rows, anchor.min(target), anchor.max(target)),
    });
    Some(target)
}

/// A wholly one-sided add/delete run remains navigable from either affinity.
/// Only an unequal replacement's filler skips the missing preferred column.
fn replacement_row(rows: &[super::diff_split::SplitDiffRow], at: usize) -> bool {
    use super::diff_split::SplitDiffRow;
    let boundary = |row: &SplitDiffRow| {
        matches!(row, SplitDiffRow::Full(_))
            || matches!(row, SplitDiffRow::Pair { left, right } if left == right)
    };
    rows[..at]
        .iter()
        .rev()
        .take_while(|row| !boundary(row))
        .chain(rows[at..].iter().take_while(|row| !boundary(row)))
        .any(|row| {
            matches!(row,
            SplitDiffRow::Pair { left: Some(left), right: Some(right) } if left != right)
        })
}

/// Navigate aligned rows, retaining unified indices for anchor and clipboard.
fn move_split_rows(
    key: SelectionKey,
    rows: &[DiffRow],
    split: &[super::diff_split::SplitDiffRow],
    action: &keyboard::SelectRows,
    distance: usize,
) -> Option<usize> {
    use super::diff_split::SplitDiffRow;
    let last = split.len().checked_sub(1)?;
    let source_last = rows.len().checked_sub(1)?;
    let mut selection = selection();
    let current = selection.as_ref().filter(|s| s.key == key);
    let cursor = current.map_or(0, |s| s.cursor.min(source_last));
    let at = super::diff_split::split_index_of(split, cursor).unwrap_or(0);
    let right = current.and_then(|s| s.split_right).unwrap_or_else(|| {
        matches!(split[at], SplitDiffRow::Pair { left, right }
            if right == Some(cursor) && left != Some(cursor))
    });
    let mut visual = if action.boundary || action.all {
        if action.step > 0 {
            last
        } else {
            0
        }
    } else if action.step > 0 {
        at.saturating_add(distance).min(last)
    } else {
        at.saturating_sub(distance)
    };
    let target = if action.all {
        source_last
    } else {
        loop {
            let target = match split[visual] {
                SplitDiffRow::Full(index) => Some(index),
                SplitDiffRow::Pair { left, right: rhs } => {
                    let preferred = if right { rhs } else { left };
                    preferred.or_else(|| {
                        (!replacement_row(split, visual))
                            .then(|| left.or(rhs))
                            .flatten()
                    })
                }
            };
            if let Some(target) = target {
                break target;
            }
            let next = if action.step > 0 {
                visual.saturating_add(1).min(last)
            } else {
                visual.saturating_sub(1)
            };
            if next == visual {
                break cursor;
            }
            visual = next;
        }
    };
    let anchor = if action.all {
        0
    } else if action.extend {
        current.map_or(0, |s| s.anchor.min(source_last))
    } else {
        target
    };
    *selection = Some(Selection {
        key,
        anchor,
        cursor: target,
        keyboard_caret: true,
        split_right: Some(right),
        text: build_text(rows, anchor.min(target), anchor.max(target)),
    });
    Some(target)
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
            line_ending: kagi_domain::diff::LineEnding::Lf,
            line_ending_changed: false,
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
        let key =
            || SelectionKey::new(SelectionOwner::Test, Default::default(), Default::default());
        let (k1, k2) = (key(), key());
        begin(k1, 2);
        assert!(drag_to(k1, 5));
        assert!(
            !is_keyboard_caret(k1, 5),
            "mouse drag retains its original header paint"
        );
        assert!(!drag_to(k2, 7), "another surface must not steal the drag");
        assert_eq!(range(k1), Some((2, 5)));
        assert_eq!(range(k2), None);
        assert!(contains(k1, 3) && !contains(k1, 6));
        set_text(k1, "x\n".into());
        assert_eq!(selected_text(k1).as_deref(), Some("x\n"));
        assert!(clear());
        assert!(selected_text(k1).is_none());
        let rows = vec![line("+a"), line(" b"), line("-c"), line("+d")];
        assert_eq!(move_rows(k1, &[], 1, true, false, false), None);
        assert_eq!(move_rows(k1, &rows, 1, true, false, false), Some(1));
        assert!(is_keyboard_caret(k1, 1));
        assert!(!is_keyboard_caret(k2, 1));
        assert_eq!(range(k1), Some((0, 1)));
        assert_eq!(selected_text(k1).as_deref(), Some("a\nb\n"));
        assert_eq!(move_rows(k1, &rows, 1, true, true, false), Some(3));
        assert_eq!(range(k1), Some((0, 3)));
        assert_eq!(move_rows(k1, &rows, -1, false, false, false), Some(2));
        assert_eq!(range(k1), Some((2, 2)));
        assert_eq!(move_rows(k1, &rows, -1, true, true, false), Some(0));
        assert_eq!(range(k1), Some((0, 2)));
        assert_eq!(move_rows(k2, &rows, 1, false, false, false), Some(1));
        assert_eq!(
            range(k1),
            None,
            "keyboard movement respects surface isolation"
        );
        assert_eq!(move_rows(k2, &rows, 1, true, true, true), Some(3));
        assert_eq!(selected_text(k2).as_deref(), Some("a\nb\nc\nd\n"));
        let other_surface =
            SelectionKey::new(SelectionOwner::Test, Default::default(), k2.revision);
        assert!(
            selected_text(other_surface).is_none(),
            "same rows in another pane are not its selection"
        );
        retire_surface(other_surface.surface);
        assert!(
            selected_text(k2).is_some(),
            "closing another pane leaves this selection intact"
        );
        let replacement = SelectionKey::new(k2.owner, k2.surface, Default::default());
        sync(replacement);
        assert!(
            selected_text(k2).is_none(),
            "same-sized content replacement retires saved code"
        );
        begin(k1, 0);
        set_text(k1, "old\n".into());
        retire_revision(k1.revision);
        assert!(range(k1).is_none());
        begin(k1, 0);
        set_text(k1, "old\n".into());
        retire_owner(k1.owner);
        assert!(selected_text(k1).is_none());
        clear();
    }
}
