//! Change-type badges for a file-history entry — the coloured `A`/`M`/`D`/`R`
//! letter every file list in kagi marks its rows with.
//!
//! Moved here from `kagi-ui-file-history` (which re-exports them as a shim)
//! so `kagi-ui-editor`'s History tab can badge its own commit rows: sibling
//! pane crates can't depend on each other (ADR-0121), only on this one.
//!
//! Note the near-twin next door: [`crate::file_tree::status_badge`] does the
//! same job for `kagi_domain::status::ChangeKind` (working-tree status), while
//! these work on `FileChangeType` (a parsed `git log` entry). Two source
//! enums, deliberately two mappings — kept side by side in this crate so the
//! letters and colours can't drift apart.

use kagi_domain::file_history::{FileChangeType, FileHistoryEntry, FileHistoryEntryKind};

use crate::theme::theme;

/// Badge for a history row: the synthetic WIP entry gets a dot, a real commit
/// gets its change letter.
pub fn entry_badge(entry: &FileHistoryEntry) -> (&'static str, u32) {
    if entry.kind == FileHistoryEntryKind::Wip {
        return ("●", theme().color_warning);
    }
    change_type_badge(entry.change.change_type)
}

/// Map a [`FileChangeType`] to its display letter + colour.
pub fn change_type_badge(ct: FileChangeType) -> (&'static str, u32) {
    let t = theme();
    match ct {
        FileChangeType::Added => ("A", t.change_added),
        FileChangeType::Modified => ("M", t.change_modified),
        FileChangeType::Deleted => ("D", t.change_deleted),
        FileChangeType::Renamed => ("R", t.change_renamed),
        // Copied has no dedicated theme colour; reuse the rename (purple/blue).
        FileChangeType::Copied => ("C", t.change_renamed),
        FileChangeType::Unknown => ("?", t.text_muted),
    }
}

/// Human label for a change type (used in the diff banner / detail pane).
pub fn change_type_label(ct: FileChangeType) -> &'static str {
    match ct {
        FileChangeType::Added => "Added",
        FileChangeType::Modified => "Modified",
        FileChangeType::Deleted => "Deleted",
        FileChangeType::Renamed => "Renamed",
        FileChangeType::Copied => "Copied",
        FileChangeType::Unknown => "Changed",
    }
}
