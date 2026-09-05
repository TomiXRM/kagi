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
//! ## Which repository a write targets (#476)
//!
//! Every write op used to resolve its repository from the **tab's**
//! `repo_path`, so a panel pointed elsewhere would have staged and committed
//! into the wrong repository; v1 (#473) made such a panel read-only outright.
//! #476 threads the panel's own path through instead, one slice at a time:
//! [`KagiApp::commit_panel_repo_path`] is the single resolver, and
//! [`KagiApp::refuse_foreign_panel_write`] stays as the guard on the ops that
//! have **not** been converted yet — the remaining call sites are the
//! migration checklist. Slice 1 converted stage / unstage; commit, amend and
//! discard-all are still tab-resolved and therefore still refused.

use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, Window};

use super::KagiApp;

/// Fold `path` to its canonical on-disk form, keeping it as spelled when it
/// cannot be resolved (a worktree removed under us).
///
/// #476: on macOS `/tmp/x` and `/private/tmp/x` are the same directory, so
/// without this a panel whose path is spelled the other way reads as "foreign"
/// — the open repository's OWN panel would be refused, and once the write ops
/// resolve from the panel they would route through a second `Backend` for a
/// repository they are already holding a session for.
pub fn canon(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// The repository a commit-panel write must target: the panel's own repository
/// when one is open, else the tab's. Pure half of
/// [`KagiApp::commit_panel_repo_path`] (#476).
pub fn panel_repo<'a>(
    tab_repo: Option<&'a Path>,
    panel_repo: Option<&'a Path>,
) -> Option<&'a Path> {
    panel_repo.or(tab_repo)
}

/// Is the open commit panel pointed at a different repository than the open
/// tab? Such a panel writes into that repository (#476) and hides the ops that
/// are still tab-resolved (see the module docs).
///
/// Pure so it can be tested without a `KagiApp`; both sides are canonicalized
/// before they get here — `open_repository` and [`canon`] in
/// `open_commit_panel_at`.
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

    /// The repository a commit-panel write op must target (#476): the panel's
    /// own — a linked worktree's, when the panel shows one — and the tab's when
    /// no panel is open. Both sides are canonical, so this resolves to exactly
    /// the repository the panel is listing files from.
    pub(crate) fn commit_panel_repo_path(&self, cx: &gpui::App) -> Option<PathBuf> {
        let panel = self
            .commit_panel
            .as_ref()
            .map(|e| e.read(cx).repo_path.clone());
        panel_repo(self.repo_path.as_deref(), panel.as_deref()).map(PathBuf::from)
    }

    /// Run `f` against a `Backend` for [`Self::commit_panel_repo_path`].
    ///
    /// The tab's own panel keeps borrowing the per-tab `RepoSession`
    /// (ADR-0107); a linked worktree's panel gets a short-lived `Backend` on
    /// its own path — the same split `diff_view`'s panel diff read uses.
    /// `None` when there is no repository to write to, or it would not open.
    pub(crate) fn with_commit_panel_repo<R>(
        &self,
        cx: &gpui::App,
        f: impl FnOnce(&kagi_git::Backend) -> R,
    ) -> Option<R> {
        let path = self.commit_panel_repo_path(cx)?;
        if self.repo_path.as_deref() == Some(path.as_path()) {
            return Some(f(self.repo_session.as_ref()?.backend()));
        }
        match kagi_git::Backend::open(&path) {
            Ok(b) => Some(f(&b)),
            Err(e) => {
                klog!("commit-panel: repo open error: {}", e);
                None
            }
        }
    }

    /// Keep a linked worktree's WIP row honest after a write into it (#476).
    ///
    /// The row's staged/unstaged counts come from the snapshot, and a full
    /// `reload` would re-snapshot AND drop the commit-panel entity
    /// (`reload.rs`), bouncing the user out of the panel they just staged from.
    /// So the one row that changed is re-read in place from the worktree
    /// itself. No-op for the tab's own repository — the watcher covers that.
    pub(crate) fn refresh_worktree_wip_row(&mut self, path: &Path) {
        if self.repo_path.as_deref() == Some(path) {
            return;
        }
        let Some(status) = kagi_git::Backend::open(path)
            .ok()
            .and_then(|b| b.working_tree_status().ok())
        else {
            return;
        };
        let wip = kagi_domain::refs::WorktreeWip {
            staged: status.staged.len(),
            unstaged: status.unstaged.len(),
            untracked: status.untracked.len(),
        };
        for wt in self.active_view.worktrees.iter_mut() {
            if canon(wt.path.clone()) == path {
                wt.wip = Some(wip);
            }
        }
    }

    /// Guard for every commit-panel write entry point: refuses `op` (and says
    /// so) while the panel belongs to another worktree AND the op still
    /// resolves its repository from the tab (#476: commit, amend, discard-all —
    /// the remaining call sites are the migration checklist). One guard in each
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

    /// #476: `/tmp/x` and `/private/tmp/x` are one directory on macOS. Folding
    /// both through [`canon`] is what stops the open repository's own panel
    /// from reading as foreign (and on Linux, where `/tmp` is real, this is
    /// simply the identity).
    #[test]
    fn canon_folds_the_two_spellings_of_one_directory() {
        let raw = std::env::temp_dir().join(format!("kagi-canon-{}", std::process::id()));
        std::fs::create_dir_all(&raw).expect("mkdir");
        let resolved = std::fs::canonicalize(&raw).expect("canonicalize");
        assert!(
            !is_foreign_panel(Some(&canon(raw.clone())), Some(&canon(resolved.clone()))),
            "canonicalized, {raw:?} and {resolved:?} are the same repository"
        );
        assert_eq!(canon(raw.clone()), resolved);
        // A path that does not exist keeps its spelling rather than vanishing.
        let gone = raw.join("removed-worktree");
        assert_eq!(canon(gone.clone()), gone);
        let _ = std::fs::remove_dir(&raw);
    }

    /// #476: the panel's repository wins; the tab's is the fallback when no
    /// panel is open. This is what points the four staging ops at the worktree.
    #[test]
    fn panel_repo_prefers_the_panel_then_the_tab() {
        let tab = Path::new("/repo");
        let wt = Path::new("/wt");
        assert_eq!(panel_repo(Some(tab), Some(wt)), Some(wt));
        assert_eq!(panel_repo(Some(tab), None), Some(tab));
        assert_eq!(panel_repo(None, Some(wt)), Some(wt));
        assert_eq!(panel_repo(None, None), None);
    }

    /// #476 slice 1 is a migration checklist: an op that still resolves its
    /// repository from the tab MUST keep the guard, and one that has been
    /// converted MUST have dropped it. Reading the op sources is the only way
    /// to assert "which ops call it" without a live `KagiApp`.
    #[test]
    fn only_the_unconverted_ops_refuse_a_foreign_panel() {
        const NEEDLE: &str = "refuse_foreign_panel_write(\"";
        let sources = [
            include_str!("operations/commit.rs"),
            include_str!("operations/discard.rs"),
        ];
        let mut guarded: Vec<&str> = sources
            .iter()
            .flat_map(|src| {
                src.match_indices(NEEDLE).map(move |(i, _)| {
                    let rest = &src[i + NEEDLE.len()..];
                    &rest[..rest.find('"').expect("unterminated op name")]
                })
            })
            .collect();
        guarded.sort_unstable();
        guarded.dedup();
        assert_eq!(
            guarded,
            ["amend", "commit", "discard-all"],
            "the set of tab-resolved (still refused) ops changed"
        );
        for converted in ["stage", "stage-all", "unstage", "unstage-all"] {
            assert!(
                !guarded.contains(&converted),
                "#476 slice 1: `{converted}` writes to the panel's repository \
                 and must not be guarded"
            );
        }
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
