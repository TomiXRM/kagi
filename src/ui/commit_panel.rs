//! Commit Panel — T025
//!
//! GitKraken 風の作業台: staging / unstaging / diff / commit message / commit button。
//! `src/git/staging.rs` (T024) の 6 API のみを使う。
//!
//! ## headless 検証 env vars
//! - `KAGI_COMMIT_PANEL=1`       起動時に Commit Panel を開き件数をログ
//! - `KAGI_STAGE_FILE=<path>`    起動時に1ファイル stage
//! - `KAGI_UNSTAGE_FILE=<path>`  起動時に1ファイル unstage
//! - `KAGI_COMMIT_MSG=<msg>`     コミットメッセージ設定 + KAGI_AUTO_CONFIRM=1 で実際にコミット

use std::path::PathBuf;

use gpui::SharedString;

use kagi::git::{Backend, ChangeKind, FileDiffStat, FileStatus};

// ──────────────────────────────────────────────────────────────
// CommitPanelFileRef — which file is selected in the panel
// ──────────────────────────────────────────────────────────────

/// Identifies a selected file in the Commit Panel: which section (staged/unstaged)
/// and its index within that section.
#[derive(Clone, Debug, PartialEq)]
pub enum CommitPanelFileRef {
    /// File is in the Unstaged section (unstaged or untracked).
    Unstaged { index: usize },
    /// File is in the Staged section.
    Staged { index: usize },
}

// ──────────────────────────────────────────────────────────────
// CommitPlanModal — plan confirmation for commit
// ──────────────────────────────────────────────────────────────

/// State for an in-progress commit plan confirmation.
#[derive(Clone)]
pub struct CommitPlanModal {
    /// The computed plan (warnings for unstaged remains, preview_files = staged).
    pub plan: std::sync::Arc<kagi::git::ops::OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CommitPanelState — all mutable state for the commit panel
// ──────────────────────────────────────────────────────────────

/// All mutable state for the Commit Panel.
///
/// Stored in `KagiApp` and reset on `reload()`.
#[derive(Clone)]
pub struct CommitPanelState {
    /// Files in the unstaged section (modified + untracked, including conflicted).
    pub unstaged: Vec<FileStatus>,
    /// Files in the staged section.
    pub staged: Vec<FileStatus>,
    /// W16-DIFFSTAT: per-file additions/deletions for unstaged files (index→WT).
    pub unstaged_stats: Vec<FileDiffStat>,
    /// W16-DIFFSTAT: per-file additions/deletions for staged files (HEAD→index).
    pub staged_stats: Vec<FileDiffStat>,
    /// Paths of conflicted files (subset of unstaged — these cannot be staged).
    pub conflicted_paths: std::collections::HashSet<PathBuf>,
    /// Currently selected file (for row highlight in the panel).
    pub selected_file: Option<CommitPanelFileRef>,
    /// Commit message text (simple String; IME fallback — T014 pattern).
    pub commit_msg: String,
    /// When Some, the commit plan confirmation modal is shown.
    pub plan_modal: Option<CommitPlanModal>,
    /// Whether the file list is in tree view (true) or flat view (false).
    pub tree_view: bool,
}

impl CommitPanelState {
    /// Create a new CommitPanelState from the current repo status.
    pub fn from_repo(repo_path: &PathBuf) -> Self {
        let mut state = CommitPanelState {
            unstaged: Vec::new(),
            staged: Vec::new(),
            unstaged_stats: Vec::new(),
            staged_stats: Vec::new(),
            conflicted_paths: std::collections::HashSet::new(),
            selected_file: None,
            commit_msg: String::new(),
            plan_modal: None,
            tree_view: false,
        };
        state.reload_status(repo_path);
        state
    }

    /// Returns true if the given unstaged file path is conflicted (cannot be staged).
    pub fn is_conflicted(&self, path: &PathBuf) -> bool {
        self.conflicted_paths.contains(path)
    }

    /// Reload unstaged/staged lists from the repository.
    pub fn reload_status(&mut self, repo_path: &PathBuf) {
        let backend = match Backend::open(repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] commit_panel: repo open error: {}", e);
                return;
            }
        };
        match backend.working_tree_status() {
            Ok(status) => {
                // Track conflicted paths for UI (these cannot be staged).
                self.conflicted_paths = status.conflicted.iter().cloned().collect();

                // Unstaged = modified + untracked combined
                let mut unstaged = status.unstaged;
                // Append untracked as Added entries
                for p in &status.untracked {
                    unstaged.push(FileStatus {
                        path: p.clone(),
                        change: ChangeKind::Added,
                    });
                }
                // Append conflicted as non-stageable entries (shown in unstaged section)
                for p in &status.conflicted {
                    unstaged.push(FileStatus {
                        path: p.clone(),
                        change: ChangeKind::Modified, // displayed with "C" badge via is_conflicted()
                    });
                }
                self.unstaged = unstaged;
                self.staged = status.staged;
                // W16-DIFFSTAT: aggregate additions/deletions for both sides.
                // Best-effort: on error leave the lists empty (bar omitted).
                self.unstaged_stats = backend.unstaged_diffstat().unwrap_or_default();
                self.staged_stats = backend.staged_diffstat().unwrap_or_default();
                // Clear selection on status change.
                self.selected_file = None;
            }
            Err(e) => {
                eprintln!("[kagi] commit_panel: working_tree_status error: {}", e);
            }
        }
    }

    /// Return true if commit is possible (staged > 0 and message non-empty).
    /// NOTE: T026 moves can_commit logic to render_commit_panel which reads InputState.
    /// This method is kept for the headless path.
    #[allow(dead_code)]
    pub fn can_commit(&self) -> bool {
        !self.staged.is_empty() && !self.commit_msg.trim().is_empty()
    }

}

// ──────────────────────────────────────────────────────────────
// Status badge helpers for staging panel
// ──────────────────────────────────────────────────────────────

/// Map a `ChangeKind` to a 1-char status badge and its colour.
/// Returns `(char, color_u32, is_conflicted)`.
pub fn status_badge(change: &ChangeKind, is_conflicted: bool) -> (&'static str, u32, bool) {
    let t = crate::ui::theme::theme();
    if is_conflicted {
        return ("C", t.color_blocker, true); // red background for conflicted
    }
    match change {
        ChangeKind::Added      => ("A", t.change_added, false),
        ChangeKind::Modified   => ("M", t.change_modified, false),
        ChangeKind::Deleted    => ("D", t.change_deleted, false),
        ChangeKind::Renamed{..} => ("R", t.change_renamed, false),
        ChangeKind::TypeChange => ("T", t.change_typechange, false),
    }
}
