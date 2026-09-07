//! Graph and branch-menu worktree navigation adapters.

use std::path::PathBuf;

use gpui::{Context, Pixels, Point};
use kagi_git::Worktree;

use super::{worktree_menu, KagiApp};

impl KagiApp {
    /// Open a worktree from its graph glyph. `open_repository` owns canonical
    /// identity and switches to an existing tab instead of duplicating it.
    pub(crate) fn open_graph_worktree(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        klog!("worktree-open: {}", path.display());
        self.open_repository(path, cx);
    }

    /// Open the shared worktree context menu from a graph badge, WIP row, or
    /// sidebar entry. Main worktrees expose no linked-worktree lifecycle ops.
    pub fn open_worktree_menu(
        &mut self,
        name: String,
        locked: bool,
        is_main: bool,
        path: Option<PathBuf>,
        position: Point<Pixels>,
    ) {
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = None;
        self.worktree_menu = Some(worktree_menu::WorktreeMenuState {
            name: name.clone(),
            locked,
            is_main,
            path,
            position,
        });
        klog!("worktree-menu: open '{}'", name);
    }
}

/// Resolve the worktree fields for a branch. The current worktree is reported
/// as checked out but deliberately has no open target because that is a no-op.
pub fn paths_for_branch(
    worktrees: &[Worktree],
    branch: Option<&str>,
) -> (Option<String>, Option<PathBuf>) {
    let worktree = branch.and_then(|branch| {
        worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some(branch))
    });
    let checked_out_path = worktree.map(|worktree| worktree.path.display().to_string());
    let open_path = worktree
        .filter(|worktree| !worktree.is_current)
        .map(|worktree| worktree.path.clone());
    (checked_out_path, open_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(path: &str, branch: Option<&str>, is_current: bool, is_main: bool) -> Worktree {
        Worktree {
            name: path.to_string(),
            path: path.into(),
            branch: branch.map(str::to_string),
            is_current,
            is_main,
            wip: None,
            head: None,
            locked: false,
            lock_reason: None,
        }
    }

    #[test]
    fn paths_cover_main_linked_current_and_detached() {
        let worktrees = vec![
            worktree("/repo", Some("main"), false, true),
            worktree("/repo-linked", Some("feature"), false, false),
            worktree("/repo-current", Some("current"), true, false),
            worktree("/repo-detached", None, false, false),
        ];

        assert_eq!(
            paths_for_branch(&worktrees, Some("main")),
            (Some("/repo".into()), Some("/repo".into()))
        );
        assert_eq!(
            paths_for_branch(&worktrees, Some("feature")),
            (Some("/repo-linked".into()), Some("/repo-linked".into()))
        );
        assert_eq!(
            paths_for_branch(&worktrees, Some("current")),
            (Some("/repo-current".into()), None)
        );
        assert_eq!(
            paths_for_branch(&worktrees, None),
            (None, None),
            "detached worktrees do not bind to a branch context"
        );
    }
}
