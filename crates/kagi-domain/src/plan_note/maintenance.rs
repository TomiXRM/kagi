//! MaintenanceNote — the repository-health fixes offered by Analyze (#358,
//! ADR-0205): writing a commit-graph and enabling the built-in fsmonitor.
//! Both only add a cache or a config key; neither touches refs, the index or
//! the working tree, so the way back is deleting what was added.

/// Plan notes for the repository-health fixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceNote {
    /// blocker (`plan_write_commit_graph`) — HEAD is unborn: there is no
    /// history to describe.
    NoCommits,
    /// blocker (`plan_enable_fsmonitor`) — `core.fsmonitor` already has a
    /// value at some config level; Kagi does not override the user's choice.
    FsmonitorAlreadySet { value: String },
    /// blocker (`plan_enable_fsmonitor`) — git's built-in fsmonitor daemon
    /// only exists on macOS and Windows.
    FsmonitorUnsupported,
    /// warning (`plan_enable_fsmonitor`) — git starts a background daemon per
    /// working tree that watches it for changes.
    FsmonitorStartsDaemon,
}

impl MaintenanceNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            MaintenanceNote::NoCommits => {
                "This repository has no commits yet, so there is no history for a \
                 commit-graph to describe."
                    .to_string()
            }
            MaintenanceNote::FsmonitorAlreadySet { value } => format!(
                "core.fsmonitor is already set to '{value}'. Kagi does not change an existing \
                 setting."
            ),
            MaintenanceNote::FsmonitorUnsupported => {
                "git's built-in filesystem monitor is only available on macOS and Windows."
                    .to_string()
            }
            MaintenanceNote::FsmonitorStartsDaemon => {
                "git will start a background daemon for this working tree that watches it for \
                 changes, so status no longer scans every file."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the repository-health fixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceTitle {
    /// `plan_write_commit_graph`.
    WriteCommitGraph,
    /// `plan_enable_fsmonitor`.
    EnableFsmonitor,
}

impl MaintenanceTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            MaintenanceTitle::WriteCommitGraph => "Write the commit-graph".to_string(),
            MaintenanceTitle::EnableFsmonitor => "Enable the filesystem monitor".to_string(),
        }
    }
}

/// How to take a repository-health fix back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceRecovery {
    /// The commit-graph is a cache: deleting its file(s) restores the
    /// previous state, and git works without it.
    WriteCommitGraph,
    /// Unset the key again.
    EnableFsmonitor,
}

impl MaintenanceRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            MaintenanceRecovery::WriteCommitGraph => {
                "The commit-graph is only a cache that git reads to walk history faster. To \
                 undo, delete it; git keeps working without it."
                    .to_string()
            }
            MaintenanceRecovery::EnableFsmonitor => {
                "To undo, remove the setting again from this repository's config.".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsmonitor_already_set_quotes_the_value() {
        assert_eq!(
            MaintenanceNote::FsmonitorAlreadySet {
                value: "false".into()
            }
            .message_en(),
            "core.fsmonitor is already set to 'false'. Kagi does not change an existing setting."
        );
    }

    #[test]
    fn titles_name_the_fix() {
        assert_eq!(
            MaintenanceTitle::WriteCommitGraph.message_en(),
            "Write the commit-graph"
        );
        assert_eq!(
            MaintenanceTitle::EnableFsmonitor.message_en(),
            "Enable the filesystem monitor"
        );
    }
}
