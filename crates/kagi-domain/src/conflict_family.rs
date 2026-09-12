//! Pure values crossing the conflict-family application boundary (#484 C1).
//!
//! Repository inspection and mutation stay in `kagi-git`; this module only
//! names immutable requests, revisions and finite evidence.

use std::path::PathBuf;

use crate::resolution::DirFileChoice;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConflictRevision(String);

impl ConflictRevision {
    pub fn from_fingerprint(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BufferRevision(String);

impl BufferRevision {
    pub fn from_fingerprint(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictDraft {
    Text(Vec<u8>),
    Raw { oid: String, mode: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictRequest {
    Save {
        path: PathBuf,
        revision: ConflictRevision,
        buffer_revision: BufferRevision,
        draft: ConflictDraft,
        operation: String,
        before_hash: String,
        actions: String,
    },
    ResolveDirFile {
        path: PathBuf,
        revision: ConflictRevision,
        choice: DirFileChoice,
    },
    /// End the whole operation and restore the pre-operation state (#704).
    ///
    /// Unlike Save and ResolveDirFile this names no path — it is about the
    /// operation, not a file — and it is admissible whenever one is in
    /// progress, resolved or not. `operation` is the slug the observation
    /// carried; the Backend refuses if the live one has become a different
    /// kind of operation.
    Abort {
        revision: ConflictRevision,
        operation: String,
    },
}

impl ConflictRequest {
    pub fn revision(&self) -> &ConflictRevision {
        match self {
            Self::Save { revision, .. }
            | Self::ResolveDirFile { revision, .. }
            | Self::Abort { revision, .. } => revision,
        }
    }

    pub fn buffer_revision(&self) -> Option<&BufferRevision> {
        match self {
            Self::Save {
                buffer_revision, ..
            } => Some(buffer_revision),
            Self::ResolveDirFile { .. } | Self::Abort { .. } => None,
        }
    }

    /// The one path this request is about. `None` for [`Self::Abort`], which
    /// is about the operation rather than a file.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Save { path, .. } | Self::ResolveDirFile { path, .. } => Some(path),
            Self::Abort { .. } => None,
        }
    }

    pub fn action(&self) -> ConflictAction {
        match self {
            Self::Save { .. } => ConflictAction::Save,
            Self::ResolveDirFile { choice, .. } => ConflictAction::ResolveDirFile(*choice),
            Self::Abort { .. } => ConflictAction::Abort,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    Save,
    ResolveDirFile(DirFileChoice),
    Abort,
}

impl ConflictAction {
    pub fn name(self) -> String {
        match self {
            Self::Save => "conflict-save".into(),
            Self::ResolveDirFile(choice) => format!("conflict-dir-file:{}", choice.slug()),
            Self::Abort => "conflict-abort".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictObservation {
    pub revision: ConflictRevision,
    pub operation: String,
    pub paths: Vec<PathBuf>,
}

/// An operation the repository is in the middle of — a merge, rebase,
/// cherry-pick, revert, or a stash apply that left conflicts.
///
/// This is an observation of the repository, present for exactly as long as
/// `.git` says the operation is: including after every conflict has been
/// resolved and nothing is unmerged any more, which is the state #704 had no
/// way out of. It is *not* derived from any view, entity or pane — those may
/// come and go while the operation stands (ADR-0196).
///
/// `Merge` is deliberately part of this and the name deliberately is not
/// "sequencer": a plain merge is one step and Git keeps no sequencer state
/// for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InProgressOperation {
    /// What the repository was observed to be doing. The same value the
    /// conflict family freezes into a request and re-reads at preflight —
    /// carried whole rather than copied field by field, so a read model and
    /// an admission decision can never be looking at different observations.
    pub observation: ConflictObservation,
    /// Sequencer position as `(step, total)`, for the operations that report
    /// one (rebase). `None` for a single-step operation.
    pub step: Option<(usize, usize)>,
}

impl InProgressOperation {
    /// `merge` / `rebase` / `cherry-pick` / `revert` / `stash`.
    pub fn slug(&self) -> &str {
        &self.observation.operation
    }

    /// The revision an abort request freezes. The Backend re-reads the live
    /// one at preflight and refuses if the two have parted.
    pub fn revision(&self) -> &ConflictRevision {
        &self.observation.revision
    }

    /// How many paths are still unmerged in the index. Zero is normal: a
    /// resolved-but-uncommitted merge is still a merge in progress.
    pub fn unmerged(&self) -> usize {
        self.observation.paths.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictProgress {
    NotStarted,
    RecoveryCaptured,
    WorktreeWritten,
    IndexWritten,
    IndexAndWorktreeWritten,
    Verified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEvidence {
    pub action: ConflictAction,
    pub progress: ConflictProgress,
    pub before: ConflictObservation,
    pub after: Option<ConflictObservation>,
    pub detail: String,
}
