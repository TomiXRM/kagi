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

use kagi::git::{Backend, ChangeKind, CommitPreview, FileDiffStat, FileStatus};

use crate::ui::file_tree::{self, TreeRow};

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
    /// Cached staged-commit preview (count / A·M·D / target branch / author),
    /// recomputed in [`Self::reload_status`]. **Must not** be recomputed every
    /// render: `commit_preview()` runs a full `working_tree_status` (~150ms on a
    /// large repo), which at 60fps froze the panel to ~6fps (PERF bug).
    pub preview: Option<CommitPreview>,
    /// PERF: cached tree rows for the unstaged section, rebuilt in
    /// [`reload_status`] so the tree is NOT recomputed every frame.
    pub unstaged_tree: Vec<TreeRow>,
    /// PERF: cached tree rows for the staged section (see `unstaged_tree`).
    pub staged_tree: Vec<TreeRow>,
    /// PERF: O(1) lookup from unstaged file path → index into `unstaged_stats`.
    /// Replaces the per-row `find_stat` linear scan (was O(N²) per frame).
    pub unstaged_stat_index: std::collections::HashMap<PathBuf, usize>,
    /// PERF: O(1) lookup from staged file path → index into `staged_stats`.
    pub staged_stat_index: std::collections::HashMap<PathBuf, usize>,
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
            preview: None,
            unstaged_tree: Vec::new(),
            staged_tree: Vec::new(),
            unstaged_stat_index: std::collections::HashMap::new(),
            staged_stat_index: std::collections::HashMap::new(),
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
                // Cache the staged-commit preview here (NOT per render frame),
                // reusing this `status` so we don't run a second
                // working_tree_status walk. Done before `status` is consumed below.
                self.preview = backend.commit_preview_from_status(&status).ok();
                // Track conflicted paths for UI (these cannot be staged).
                self.conflicted_paths = status.conflicted.iter().cloned().collect();

                // Whether there are tracked modifications (the only thing
                // unstaged_diffstat covers) — captured before `status` is moved.
                let has_tracked_modifications = !status.unstaged.is_empty();

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
                // unstaged_diffstat covers tracked modifications only — skip the
                // (working-tree-walking) call entirely when there are none, so a
                // dir full of untracked files costs nothing here.
                self.unstaged_stats = if has_tracked_modifications {
                    backend.unstaged_diffstat().unwrap_or_default()
                } else {
                    Vec::new()
                };
                self.staged_stats = backend.staged_diffstat().unwrap_or_default();
                // Clear selection on status change.
                self.selected_file = None;
                // PERF: recompute the cached tree rows and diffstat indices once
                // per status change (NOT per frame).
                self.rebuild_derived();
            }
            Err(e) => {
                eprintln!("[kagi] commit_panel: working_tree_status error: {}", e);
            }
        }
    }

    /// PERF: rebuild the cached tree rows and diffstat path→index maps from the
    /// current `unstaged`/`staged`/`*_stats` lists.  Called once per status
    /// change from [`reload_status`], so render is O(visible rows) not O(N²).
    fn rebuild_derived(&mut self) {
        self.unstaged_tree = file_tree::build_file_tree(&self.unstaged);
        self.staged_tree = file_tree::build_file_tree(&self.staged);

        self.unstaged_stat_index = self
            .unstaged_stats
            .iter()
            .enumerate()
            .map(|(i, s)| (s.path.clone(), i))
            .collect();
        self.staged_stat_index = self
            .staged_stats
            .iter()
            .enumerate()
            .map(|(i, s)| (s.path.clone(), i))
            .collect();
    }

    /// O(1) lookup of the unstaged [`FileDiffStat`] for `path`.
    pub fn unstaged_stat(&self, path: &PathBuf) -> Option<&FileDiffStat> {
        self.unstaged_stat_index
            .get(path)
            .and_then(|&i| self.unstaged_stats.get(i))
    }

    /// O(1) lookup of the staged [`FileDiffStat`] for `path`.
    pub fn staged_stat(&self, path: &PathBuf) -> Option<&FileDiffStat> {
        self.staged_stat_index
            .get(path)
            .and_then(|&i| self.staged_stats.get(i))
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
        ChangeKind::Added => ("A", t.change_added, false),
        ChangeKind::Modified => ("M", t.change_modified, false),
        ChangeKind::Deleted => ("D", t.change_deleted, false),
        ChangeKind::Renamed { .. } => ("R", t.change_renamed, false),
        ChangeKind::TypeChange => ("T", t.change_typechange, false),
    }
}
