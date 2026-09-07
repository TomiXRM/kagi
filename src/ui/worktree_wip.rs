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
//! #476 threaded the panel's own path through instead, one slice at a time:
//! [`KagiApp::write_repo_path`] is the single resolver, and every write says
//! which view it came from ([`WriteOrigin`]) — the commit panel follows the
//! panel, the Editor Workspace tree always follows the tab. Slice 1
//! converted stage / unstage, slice 2 commit (including the message inputs
//! that feed it — see [`draft_branch`]), slice 3 amend and discard. With the
//! last op converted the `refuse_foreign_panel_write` guard is **gone**: no
//! commit-panel write resolves the tab any more, so there is nothing left to
//! refuse. [`is_foreign_panel`] survives — it still drives the header chip and
//! [`KagiApp::refresh_worktree_wip_row`].
//!
//! ## Undo stays inside one repository (#476 slice 3)
//!
//! `operation_history` (Cmd+Z) is **per tab**: an entry is `(branch, before,
//! after)` and undoing it moves that branch in the tab's repository. An op
//! that ran in a linked worktree therefore records **no** entry —
//! [`KagiApp::undo_skipped_for_foreign`] is the single place that decision is
//! made, and it says so on stderr. The oplog entry, which carries the
//! worktree's own path, stays the recovery handle for those ops.
//!
//! Cross-repository undo is deliberately out of scope: making it work needs a
//! per-repository history stack (today there is one, keyed to nothing) plus a
//! rule for what the tab's Cmd+Z means when the last op was elsewhere. Both
//! are design decisions, not plumbing, and getting them wrong moves a branch
//! in a repository the user is not looking at.

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

/// Which view a write was dispatched from — and therefore which repository it
/// must target (#476 slice 3 review).
///
/// The commit panel can point at a linked worktree, so its writes follow the
/// panel. Every other view lists the **tab's** files and must keep resolving
/// the tab, whatever the panel happens to be showing: the Editor Workspace
/// tree's "Discard Changes…" shares `open_discard_modal_for_path` with the
/// panel's file menu, and without this it would discard a linked worktree's
/// copy of the same relative path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOrigin {
    /// The commit panel (its header buttons, file rows, footer, file menu).
    CommitPanel,
    /// The Editor Workspace file tree — always the open tab's repository.
    EditorTree,
}

/// The repository a write from `origin` must target. Pure half of
/// [`KagiApp::write_repo_path`] (#476).
pub fn write_repo_for<'a>(
    origin: WriteOrigin,
    tab_repo: Option<&'a Path>,
    panel_repo: Option<&'a Path>,
) -> Option<&'a Path> {
    match origin {
        WriteOrigin::CommitPanel => panel_repo.or(tab_repo),
        WriteOrigin::EditorTree => tab_repo,
    }
}

/// The branch a commit panel's message draft is keyed by (#476 slice 2).
///
/// Drafts are stored under `sha1(repo_path \0 branch)` (`kagi_git::drafts`), so
/// the panel's own path already keeps two repositories' drafts apart. The
/// branch has to follow as well: a panel showing a linked worktree is on that
/// worktree's branch, not the tab's, and keying its draft by the tab's branch
/// would strand the draft the moment the tab checked out something else.
/// `foreign_label` is the worktree chip's text (its branch, or its name when
/// detached) — the same string the WIP row is drawn with.
pub fn draft_branch(foreign_label: Option<&str>, tab_branch: &str) -> String {
    foreign_label.unwrap_or(tab_branch).to_string()
}

/// A destructive modal's title, naming the linked worktree it will act on
/// (#476 slice 3).
///
/// Amend rewrites history and discard destroys working-tree content; a title
/// that says only *what* is about to happen leaves *where* to the header chip
/// behind the modal's own backdrop. `worktree` is the panel's chip label (the
/// worktree's branch, or its name when detached) — `None` for the tab's own
/// repository, which needs no disambiguation.
pub fn worktree_modal_title(base: &str, worktree: Option<&str>) -> String {
    match worktree {
        None => base.to_string(),
        Some(label) => format!("{base} — {} {label}", super::i18n::Msg::InWorktree.t()),
    }
}

/// Is the open commit panel pointed at a different repository than the open
/// tab? Such a panel writes into that repository (#476); the header chip and
/// the WIP-row refresh follow from it (see the module docs).
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

    /// The repository a write dispatched from `origin` must target (#476) —
    /// see [`write_repo_for`]. Both sides are canonical, so a commit-panel
    /// write resolves to exactly the repository the panel is listing files
    /// from, and an editor-tree write to the tab's.
    pub(crate) fn write_repo_path(&self, origin: WriteOrigin, cx: &gpui::App) -> Option<PathBuf> {
        let panel = self
            .commit_panel
            .as_ref()
            .map(|e| e.read(cx).repo_path.clone());
        write_repo_for(origin, self.repo_path.as_deref(), panel.as_deref()).map(PathBuf::from)
    }

    /// [`Self::write_repo_path`] for the commit panel — the origin all but one
    /// write entry point has.
    pub(crate) fn commit_panel_repo_path(&self, cx: &gpui::App) -> Option<PathBuf> {
        self.write_repo_path(WriteOrigin::CommitPanel, cx)
    }

    /// The draft key's branch for the open commit panel — see [`draft_branch`].
    pub(crate) fn panel_draft_branch(&self, cx: &gpui::App) -> String {
        let panel = self.commit_panel.as_ref().map(|e| e.read(cx));
        let label = panel.and_then(|v| v.foreign.as_ref().map(|(l, _)| l.to_string()));
        draft_branch(label.as_deref(), &self.view().status_summary.branch)
    }

    /// Run `f` against a `Backend` for [`Self::write_repo_path`].
    ///
    /// The tab's own repository keeps borrowing the per-tab `RepoSession`
    /// (ADR-0107); a linked worktree's panel gets a short-lived `Backend` on
    /// its own path — the same split `diff_view`'s panel diff read uses.
    /// `None` when there is no repository to write to, or it would not open.
    pub(crate) fn with_write_repo<R>(
        &self,
        origin: WriteOrigin,
        cx: &gpui::App,
        f: impl FnOnce(&kagi_git::Backend) -> R,
    ) -> Option<R> {
        let path = self.write_repo_path(origin, cx)?;
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

    /// [`Self::with_write_repo`] for the commit panel.
    pub(crate) fn with_commit_panel_repo<R>(
        &self,
        cx: &gpui::App,
        f: impl FnOnce(&kagi_git::Backend) -> R,
    ) -> Option<R> {
        self.with_write_repo(WriteOrigin::CommitPanel, cx, f)
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
        for wt in self.view_mut().worktrees.iter_mut() {
            if canon(wt.path.clone()) == path {
                wt.wip = Some(wip);
            }
        }
    }

    /// The open commit panel's worktree chip label, when it shows a linked
    /// worktree (#476 slice 3). `None` for the tab's own panel, or no panel.
    /// Feeds [`worktree_modal_title`] so a destructive confirm names the
    /// repository it is about to rewrite.
    pub(crate) fn panel_worktree_label(&self, cx: &gpui::App) -> Option<SharedString> {
        self.commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).foreign.as_ref().map(|(l, _)| l.clone()))
    }

    /// Must the tab's undo stack ignore an op that just ran in `repo_path`?
    ///
    /// #476 slice 3: `operation_history` is per **tab**, and an entry is
    /// `(branch, before, after)` applied to the tab's repository. An op that
    /// ran in a linked worktree must therefore record nothing — otherwise the
    /// tab's Cmd+Z would move the tab's branch to a SHA that belongs to
    /// another working tree. The oplog entry, which carries the worktree's own
    /// path, stays the recovery handle. See the module docs for why
    /// cross-repository undo is out of scope.
    pub(crate) fn undo_skipped_for_foreign(&self, op: &str, repo_path: &Path) -> bool {
        if self.repo_path.as_deref() == Some(repo_path) {
            return false;
        }
        klog!(
            "undo: skipped — {} ran in another worktree {}",
            op,
            repo_path.display()
        );
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
        build_worktree_menu(locked, false, path)
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

    /// #476: a commit-panel write follows the panel (the tab is the fallback
    /// when no panel is open) — that is what points stage/commit/amend/discard
    /// at the worktree. An **editor-tree** write follows the tab, always: that
    /// tree lists the tab's files, and resolving the panel would discard a
    /// linked worktree's copy of the same relative path (slice 3 review).
    #[test]
    fn write_repo_follows_the_origin() {
        use WriteOrigin::*;
        let tab = Path::new("/repo");
        let wt = Path::new("/wt");
        // (origin, tab, panel) → expected
        let cases = [
            (CommitPanel, Some(tab), Some(wt), Some(wt)),
            (CommitPanel, Some(tab), None, Some(tab)),
            (CommitPanel, None, Some(wt), Some(wt)),
            (CommitPanel, None, None, None),
            (EditorTree, Some(tab), Some(wt), Some(tab)),
            (EditorTree, Some(tab), None, Some(tab)),
            (EditorTree, None, Some(wt), None),
            (EditorTree, None, None, None),
        ];
        for (origin, tab_repo, panel, want) in cases {
            assert_eq!(
                write_repo_for(origin, tab_repo, panel),
                want,
                "{origin:?} with tab={tab_repo:?} panel={panel:?}"
            );
        }
    }

    /// #476: the panel's repository is where its draft lives; a foreign panel's
    /// branch is the worktree's, not the tab's.
    #[test]
    fn draft_branch_follows_the_panel() {
        assert_eq!(draft_branch(None, "main"), "main");
        assert_eq!(draft_branch(Some("ahead"), "main"), "ahead");
    }

    /// #476 slice 3 retires the guard: with amend and discard converted, every
    /// commit-panel write resolves [`KagiApp::commit_panel_repo_path`], so
    /// there is nothing left for a read-only refusal to protect. This is the
    /// migration checklist's terminal state — the count must stay **zero**, and
    /// a re-introduced guard means an op went back to resolving the tab.
    #[test]
    fn no_write_op_refuses_a_foreign_panel_any_more() {
        const NEEDLE: &str = "refuse_foreign_panel_write";
        // The op sources only — this module's own prose still names the retired
        // guard, and a scan that reads itself can never go to zero.
        let sources = [
            include_str!("operations/commit.rs"),
            include_str!("operations/discard.rs"),
            include_str!("operations/history.rs"),
        ];
        let guarded: usize = sources.iter().map(|src| src.matches(NEEDLE).count()).sum();
        assert_eq!(
            guarded, 0,
            "#476 slice 3 deleted `refuse_foreign_panel_write`; a call site means \
             a write op resolves the TAB's repository again"
        );
    }

    /// #476 slice 3: a destructive confirm names the worktree it rewrites. The
    /// tab's own panel has no label and keeps the plain title.
    #[test]
    fn a_worktree_modal_title_names_the_worktree() {
        assert_eq!(worktree_modal_title("Amend", None), "Amend");
        let named = worktree_modal_title("Amend", Some("wt-a"));
        assert!(
            named.starts_with("Amend") && named.contains("wt-a"),
            "the title must keep the op and name the worktree, got {named:?}"
        );
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

    #[test]
    fn detached_main_badge_menu_keeps_path_actions_but_not_linked_lifecycle() {
        let actions: Vec<_> = build_worktree_menu(false, true, Some(Path::new("/main")))
            .into_iter()
            .flat_map(|group| group.items.into_iter().map(|item| item.action))
            .collect();
        assert!(actions.contains(&WorktreeAction::OpenInNewTab));
        assert!(actions.contains(&WorktreeAction::Reveal));
        assert!(actions.contains(&WorktreeAction::CopyPath));
        assert!(actions.contains(&WorktreeAction::Prune));
        assert!(!actions.contains(&WorktreeAction::Lock));
        assert!(!actions.contains(&WorktreeAction::Remove {
            delete_branch: false,
        }));
    }
}
