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

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

// Private serde DTO: the public recovery model has no persistence policy.
#[derive(Serialize, Deserialize)]
#[serde(remote = "RecoveryHandle")]
struct RecoveryRecord {
    kind: String,
    oid: String,
    #[serde(default, deserialize_with = "optional_string")]
    path: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    reference: Option<String>,
}

struct RecoveryRef<'a>(&'a RecoveryHandle);

impl Serialize for RecoveryRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RecoveryRecord::serialize(self.0, serializer)
    }
}

pub(super) fn serialize<S: Serializer>(
    handles: &[RecoveryHandle],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(handles.iter().map(RecoveryRef))
}

fn optional_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(text) => Some(text),
        _ => None,
    })
}

/// Missing/malformed additive data never invents recovery from display prose.
/// Invalid members are ignored without discarding valid sibling handles.
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<RecoveryHandle>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter(|item| item.is_object())
            .filter_map(|item| RecoveryRecord::deserialize(item).ok())
            .collect(),
        _ => Vec::new(),
    })
}
