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
    },
    ResolveDirFile {
        path: PathBuf,
        revision: ConflictRevision,
        choice: DirFileChoice,
    },
}

impl ConflictRequest {
    pub fn revision(&self) -> &ConflictRevision {
        match self {
            Self::Save { revision, .. } | Self::ResolveDirFile { revision, .. } => revision,
        }
    }

    pub fn buffer_revision(&self) -> Option<&BufferRevision> {
        match self {
            Self::Save {
                buffer_revision, ..
            } => Some(buffer_revision),
            Self::ResolveDirFile { .. } => None,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Save { path, .. } | Self::ResolveDirFile { path, .. } => path,
        }
    }

    pub fn action(&self) -> ConflictAction {
        match self {
            Self::Save { .. } => ConflictAction::Save,
            Self::ResolveDirFile { choice, .. } => ConflictAction::ResolveDirFile(*choice),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    Save,
    ResolveDirFile(DirFileChoice),
}

impl ConflictAction {
    pub fn name(self) -> String {
        match self {
            Self::Save => "conflict-save".into(),
            Self::ResolveDirFile(choice) => format!("conflict-dir-file:{}", choice.slug()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictObservation {
    pub revision: ConflictRevision,
    pub operation: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictProgress {
    NotStarted,
    WorktreeWritten,
    IndexWritten,
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
