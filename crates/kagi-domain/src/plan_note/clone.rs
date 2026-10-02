//! CloneNote — cloning a GitHub repository into a new folder (#923).
//!
//! The one write that starts with no repository: the plan is about a source
//! (`[host/]owner/repo`) and a destination folder, and its only effect is
//! that folder. Kagi never deletes it, whatever happens to the clone.

/// Plan notes for the clone op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneNote {
    /// blocker — the source is not a `[host/]owner/repo` identity Kagi can
    /// hand to `gh repo clone`.
    SourceInvalid { source: String },
    /// blocker — the destination is not an absolute path.
    DestinationNotAbsolute { path: String },
    /// blocker — the destination path is not valid UTF-8, so the name Kagi
    /// shows and records for it would not be the path itself.
    DestinationNotUtf8 { path: String },
    /// blocker — something already exists at the destination and it is not
    /// an empty folder (a file, a symlink, or a folder with entries). Nothing
    /// is overwritten.
    DestinationNotEmpty { path: String },
    /// blocker — the folder the destination would be created in is missing.
    ParentMissing { path: String },
    /// blocker — the destination could not be inspected at all.
    DestinationUnreadable { path: String, error: String },
    /// blocker — the clone about to run is not the one the confirmed plan
    /// showed (another source or destination). Nothing is cloned.
    PlanMismatch { source: String, path: String },
    /// warning — the source is a fork: `gh` also adds an `upstream` remote
    /// pointing at the repository it was forked from.
    ForkAddsUpstream,
}

impl CloneNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            CloneNote::SourceInvalid { source } => {
                format!("'{source}' is not a GitHub repository Kagi can clone.")
            }
            CloneNote::DestinationNotAbsolute { path } => {
                format!("The destination '{path}' is not an absolute path.")
            }
            CloneNote::DestinationNotUtf8 { path } => format!(
                "The destination '{path}' contains characters that are not valid UTF-8. \
                 Choose a destination with a UTF-8 name."
            ),
            CloneNote::DestinationNotEmpty { path } => format!(
                "'{path}' already exists and is not an empty folder. Choose another \
                 destination; Kagi does not overwrite it."
            ),
            CloneNote::ParentMissing { path } => {
                format!("The folder '{path}' does not exist.")
            }
            CloneNote::DestinationUnreadable { path, error } => {
                format!("'{path}' could not be checked: {error}")
            }
            CloneNote::PlanMismatch { source, path } => format!(
                "Cloning '{source}' into '{path}' is not what was reviewed. Review the \
                 clone again."
            ),
            CloneNote::ForkAddsUpstream => "This repository is a fork: gh also adds an \
                 'upstream' remote for the repository it was forked from."
                .to_string(),
        }
    }
}

/// Plan titles for the clone op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneTitle {
    /// `plan_clone` — `Clone <source>`.
    Clone { source: String },
}

impl CloneTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            CloneTitle::Clone { source } => format!("Clone {source}"),
        }
    }
}
