//! Typed recovery handles (#500).
//!
//! Savepoint OIDs, dropped-stash OIDs and path→blob backup maps used to exist
//! only inside the English `after.dirty` sentence, so a consumer that wanted to
//! *recover* had to parse display text. They are now recorded as their own
//! additive `recovery` array, exactly the way `backup_refs` was added by
//! ADR-0179: the summary stays byte-identical for display, and legacy lines
//! that carry only the prose read back as an empty list — never as a claim that
//! something is recoverable.
//!
//! `kind` is a plain string for the same reason [`super::OpLogEntry::op`] is:
//! a tag written by a newer Kagi must round-trip through an older reader
//! instead of collapsing into a wrong variant.

use super::escape_json_string;

/// Pre-restore savepoint of the overwritten working tree (`OperationOutcome::RestoreSnapshot`).
pub const SAVEPOINT: &str = "savepoint";
/// Full commit OID of a dropped stash, recoverable with `git stash store`.
pub const STASH: &str = "stash";
/// One backed-up working-tree file: `path` + its ODB blob (+ its GC root ref).
pub const FILE_BACKUP: &str = "file-backup";
/// Tip of a branch that was deleted or is about to disappear with a worktree.
pub const BRANCH_TIP: &str = "branch-tip";
/// Where a history move started / ended (`undo-*` / `redo-*`).
pub const HISTORY_FROM: &str = "history-from";
pub const HISTORY_TO: &str = "history-to";

/// One recovery handle: an OID a consumer can act on, plus the repo-relative
/// path it backs up and the ref that pins it, when those apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryHandle {
    /// What the OID is — one of the `kind` constants in this module.
    pub kind: String,
    /// Full (40-hex) blob or commit OID.
    pub oid: String,
    /// Repo-relative path this OID holds the content of, for per-file handles.
    pub path: Option<String>,
    /// GC reachability root pinning `oid` (ADR-0179), when one was created.
    pub reference: Option<String>,
}

impl RecoveryHandle {
    /// A whole-repository handle (savepoint, stash, branch tip).
    pub fn oid(kind: &str, oid: impl Into<String>) -> Self {
        RecoveryHandle {
            kind: kind.to_string(),
            oid: oid.into(),
            path: None,
            reference: None,
        }
    }

    /// A per-file handle: `path` → `blob`, optionally pinned by `reference`.
    pub fn file(
        path: impl Into<String>,
        blob: impl Into<String>,
        reference: Option<String>,
    ) -> Self {
        RecoveryHandle {
            kind: FILE_BACKUP.to_string(),
            oid: blob.into(),
            path: Some(path.into()),
            reference,
        }
    }

    /// Builder: pin this handle to the ref that keeps `oid` reachable.
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }
}

/// The elements of the `"recovery":[…]` array, comma-joined (no brackets).
///
/// Every value is a JSON string, so a path containing `,`, `=` or non-ASCII
/// stays unambiguous — the ambiguity of the comma-joined `path=blob` summary
/// is exactly what #500 is about.
pub(super) fn to_json(handles: &[RecoveryHandle]) -> String {
    handles
        .iter()
        .map(|h| {
            format!(
                "{{\"kind\":{},\"oid\":{},\"path\":{},\"reference\":{}}}",
                escape_json_string(&h.kind),
                escape_json_string(&h.oid),
                optional(h.path.as_deref()),
                optional(h.reference.as_deref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn optional(value: Option<&str>) -> String {
    value.map(escape_json_string).unwrap_or("null".to_string())
}

/// Read the additive `recovery` array off a JSONL line.
///
/// Anything that does not fit the shape — a pre-#500 line, a `recovery` key
/// that is not an array of `{kind, oid}` objects — reads as *no typed data*.
/// The prose summary is never mined for a substitute: an entry that predates
/// this field is not retroactively recoverable.
pub(super) fn parse(line: &str) -> Vec<RecoveryHandle> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    let Some(items) = value.get("recovery").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(RecoveryHandle {
                kind: item.get("kind")?.as_str()?.to_string(),
                oid: item.get("oid")?.as_str()?.to_string(),
                path: item.get("path").and_then(|v| v.as_str()).map(str::to_owned),
                reference: item
                    .get("reference")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            })
        })
        .collect()
}
