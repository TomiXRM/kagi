//! issue #473 — a linked worktree's WIP row, in place.
//!
//! Clicking a linked worktree's WIP row used to call `open_repository(path)`:
//! a new tab plus a full graph snapshot, which on a 10k-commit repo costs
//! seconds for a question ("what changed over there?") that only needs
//! `working_tree_status`. So the click now builds the **commit panel** against
//! the worktree's path instead — `CommitPanelState::from_repo` is a
//! `Backend::open` + status read, no snapshot — and "open it as a tab" is
//! demoted to the row's context menu.
//!
//! ## v1 is read-only (PM decision)
//!
//! Every write op (`start_commit`, the staging pair, discard, amend) resolves
//! its repository from the **tab's** `repo_path`, so a panel pointed elsewhere
//! would stage and commit into the wrong repository. Rather than thread the
//! panel's path through `plan → confirm → preflight → execute → verify →
//! oplog` here, v1 makes such a panel read-only: the controls are hidden and
//! [`KagiApp::commit_panel_is_foreign`] guards every write entry point.
//! Threading the path through is the follow-up.

use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, Window};

use super::KagiApp;

/// Is the open commit panel pointed at a different repository than the open
/// tab? Such a panel is **read-only** (see the module docs).
///
/// Pure so it can be tested without a `KagiApp`; both sides are already
/// canonicalized by `open_repository` / the snapshot's worktree list.
pub fn is_foreign_panel(tab_repo: Option<&Path>, panel_repo: Option<&Path>) -> bool {
    match (tab_repo, panel_repo) {
        (Some(tab), Some(panel)) => tab != panel,
        // No panel, or no open repo: nothing foreign to protect against.
        _ => false,
    }
}

impl KagiApp {
    /// Whether the open commit panel belongs to another repository (a linked
    /// worktree). Guards every write op — see the module docs.
    pub fn commit_panel_is_foreign(&self, cx: &gpui::App) -> bool {
        is_foreign_panel(
            self.repo_path.as_deref(),
            self.commit_panel
                .as_ref()
                .map(|e| e.read(cx).repo_path.clone())
                .as_deref(),
        )
    }

    /// Guard for every commit-panel write entry point: refuses `op` (and says
    /// so) while the panel belongs to another worktree. One guard in each
    /// `KagiApp` method rather than in the panel's render, so a write is blocked
    /// however it was reached — a keybinding, the command palette, or a future
    /// caller that never saw the hidden buttons.
    pub(crate) fn refuse_foreign_panel_write(&mut self, op: &str, cx: &gpui::App) -> bool {
        if !self.commit_panel_is_foreign(cx) {
            return false;
        }
        klog!("refused: {} — worktree panel is read-only", op);
        self.status_footer = super::FooterStatus::Idle(SharedString::from(
            super::i18n::Msg::WorktreePanelReadOnly.t(),
        ));
        true
    }

    /// Show a linked worktree's uncommitted changes in the commit panel,
    /// **without** opening a tab for it (#473). `label` / `color_idx` are the
    /// WIP row's own chip text and lane colour, so the panel header names the
    /// worktree in the same colour the row was drawn in.
    ///
    /// The FS watcher only watches the open working tree (`src/ui/watcher.rs`),
    /// so this panel does NOT auto-refresh: re-clicking the row (or a refresh)
    /// reloads it. A second watcher for a foreign path is the follow-up.
    pub fn open_commit_panel_for_worktree(
        &mut self,
        path: PathBuf,
        label: SharedString,
        color_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        klog!("commit-panel: worktree {}", path.display());
        self.open_commit_panel_at(path, Some((label, color_idx)), window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::worktree_menu::{build_worktree_menu, WorktreeAction};
    use super::*;

    fn actions(locked: bool, path: Option<&Path>) -> Vec<WorktreeAction> {
        build_worktree_menu(locked, path)
            .into_iter()
            .flat_map(|g| g.items.into_iter().map(|i| i.action))
            .collect()
    }

    #[test]
    fn foreign_panel_needs_two_different_paths() {
        let a = Path::new("/repo");
        let b = Path::new("/repo/../wt");
        assert!(is_foreign_panel(Some(a), Some(b)));
        assert!(!is_foreign_panel(Some(a), Some(a)));
        // A panel with no repo, or no repo open at all, is never "foreign":
        // the write guards must not fire on the ordinary no-panel path.
        assert!(!is_foreign_panel(Some(a), None));
        assert!(!is_foreign_panel(None, Some(a)));
        assert!(!is_foreign_panel(None, None));
    }

    #[test]
    fn wip_row_menu_adds_the_three_path_items() {
        let with_path = actions(false, Some(Path::new("/wt")));
        assert!(with_path.contains(&WorktreeAction::OpenInNewTab));
        assert!(with_path.contains(&WorktreeAction::Reveal));
        assert!(with_path.contains(&WorktreeAction::CopyPath));
        // …on top of, not instead of, the lifecycle items the sidebar shows.
        assert!(with_path.contains(&WorktreeAction::Lock));
        assert!(with_path.contains(&WorktreeAction::Prune));
        assert!(with_path.contains(&WorktreeAction::Remove {
            delete_branch: false
        }));
    }

    #[test]
    fn sidebar_menu_is_unchanged_without_a_path() {
        let no_path = actions(false, None);
        assert!(!no_path.contains(&WorktreeAction::OpenInNewTab));
        assert!(!no_path.contains(&WorktreeAction::Reveal));
        assert!(!no_path.contains(&WorktreeAction::CopyPath));
        assert_eq!(no_path.len(), 6, "sidebar menu grew: {no_path:?}");
    }
}
