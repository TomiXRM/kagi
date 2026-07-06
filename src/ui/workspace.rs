//! Workspace pane framework (ADR-0120).
//!
//! The single place that decides **what each layout slot shows**. Before this
//! module, the precedence between the scattered gate fields (`file_history`,
//! `ecosystem`, `loading_tab`, `main_diff`, `commit_panel_open`,
//! `inspector_visible`, `sidebar.visible`, …) was implicit in the if/else
//! ordering inside `render_body`. Adding a new pane content meant finding the
//! right branch by archaeology.
//!
//! Now: `render_body` snapshots the gates into [`WorkspaceInputs`], calls
//! [`resolve_workspace`], and routes each slot on the returned
//! [`WorkspaceLayout`]. Adding a new pane content = a new enum variant here +
//! a render arm in `render_body` (+ an open/close method on `KagiApp`).
//!
//! One case pushes rather than just routes: the `CenterPane::Editor` arm is a
//! single self-rendering entity (re-entrancy — see `editor_workspace.rs`), so
//! it can't be skipped slot-by-slot the way the sidebar/right-panel arms are.
//! Instead `render_body` pushes `layout.left` into the entity's `show_tree`
//! field before embedding it (same push-then-embed pattern as the Commit
//! Panel's `active_wip`/`panel_render_width` below), so the resolved layout
//! stays the single source of truth even though the render call itself can't
//! branch on it (T-WS-EDITOR-005 finding #3).
//!
//! Scope notes:
//! - The **Conflict Mode** body and the error/welcome screens are gated one
//!   level above (in `render.rs`) because they replace the whole body
//!   including the sidebar and the bottom panel; they are documented in
//!   ADR-0120 but deliberately not routed through this resolver.
//! - The **bottom panel** keeps its own `BottomTab` enum (`types.rs`) — it is
//!   already an explicit slot switch and doesn't interact with these slots.
//! - This is pure slot *policy* (unit-tested below). It stays in `src/ui/`
//!   rather than `kagi-domain` because its vocabulary (Inspector, CommitPanel,
//!   Navigator) is UI, not Git domain.

/// What the left pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPane {
    /// Repository Navigator (branches / remotes / tags / stashes / worktrees).
    Navigator,
    /// Working-tree changed-file tree (Editor mode, T-WS-EDITOR-001).
    FileTree,
    /// Sidebar toggled off (View → Toggle Sidebar).
    Hidden,
}

/// What the center (main) pane shows. Order of the variants documents the
/// precedence: earlier wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterPane {
    /// File History takeover (ADR-0089/0117) — spans center + right.
    FileHistory,
    /// Code Ecosystem / Analyze takeover (ADR-0119) — spans center + right.
    Ecosystem,
    /// `Loading <repo>…` placeholder during an uncached tab open (W6-TABSPEED).
    Loading,
    /// Read-only code viewer (Editor mode, T-WS-EDITOR-001).
    /// Beats `Diff` — in Editor mode `main_diff` is ignored.
    Editor,
    /// Full-width diff view (T-UI-003).
    Diff,
    /// The commit graph list (default).
    CommitList,
}

/// What the right pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPane {
    /// Staging + commit message panel (T025 / ADR-0118).
    CommitPanel,
    /// Commit Inspector (W2-INSPECTOR).
    Inspector,
    /// The selected file's WIP hunks (Editor mode, T-WS-EDITOR-001).
    Hunks,
    /// No right panel (and no divider).
    Hidden,
}

/// Resolved slot contents for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub left: LeftPane,
    pub center: CenterPane,
    pub right: RightPane,
}

/// Snapshot of the `KagiApp` gate fields the resolver needs. Plain bools so
/// the precedence policy is unit-testable without gpui.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceInputs {
    /// `sidebar.visible` (View → Toggle Sidebar).
    pub sidebar_visible: bool,
    /// `file_history.is_some()`.
    pub file_history_open: bool,
    /// `ecosystem.is_some()`.
    pub ecosystem_open: bool,
    /// `loading_tab.is_some()`.
    pub loading: bool,
    /// `main_diff.is_some()`.
    pub diff_open: bool,
    /// `commit_panel_open` (the visibility gate; set by WIP-row click).
    pub commit_panel_open: bool,
    /// `commit_panel.is_some()` (the entity exists).
    pub commit_panel_present: bool,
    /// `inspector_visible` (View → Toggle Commit Details).
    pub inspector_visible: bool,
    /// A commit detail was resolved for the current selection.
    pub has_detail: bool,
    /// `editor_workspace.is_some()` (T-WS-EDITOR-001; derived, not a separate
    /// mode field — T-WS-EDITOR-005 finding #11). The most-upstream input:
    /// still beaten by the FileHistory/Ecosystem takeovers and by Loading,
    /// but beats `Diff` (Editor mode ignores `main_diff`) and overrides the
    /// right pane's CommitPanel/Inspector.
    pub editor_mode: bool,
}

/// Resolve the slot contents. This encodes, in one place, the precedence that
/// used to live in `render_body`'s branch ordering:
///
/// - center: FileHistory > Ecosystem > Loading > Editor > Diff > CommitList
/// - right:  hidden under a takeover; else Hunks (Editor mode) > CommitPanel
///   (when open AND the entity exists) > Inspector (when visible AND a detail
///   resolved) > Hidden. `commit_panel_open` without an entity hides the panel
///   *without* falling back to the Inspector (pre-existing behavior, kept).
/// - left:   FileTree (Editor mode) or Navigator, unless toggled off
///   (independent of the center mode — takeovers replace center+right only).
pub fn resolve_workspace(i: &WorkspaceInputs) -> WorkspaceLayout {
    let center = if i.file_history_open {
        CenterPane::FileHistory
    } else if i.ecosystem_open {
        CenterPane::Ecosystem
    } else if i.loading {
        CenterPane::Loading
    } else if i.editor_mode {
        CenterPane::Editor
    } else if i.diff_open {
        CenterPane::Diff
    } else {
        CenterPane::CommitList
    };

    let left = if !i.sidebar_visible {
        LeftPane::Hidden
    } else if i.editor_mode {
        LeftPane::FileTree
    } else {
        LeftPane::Navigator
    };

    let takeover = matches!(center, CenterPane::FileHistory | CenterPane::Ecosystem);
    let right = if takeover {
        RightPane::Hidden
    } else if i.editor_mode {
        RightPane::Hunks
    } else if i.commit_panel_open {
        if i.commit_panel_present {
            RightPane::CommitPanel
        } else {
            RightPane::Hidden
        }
    } else if i.inspector_visible && i.has_detail {
        RightPane::Inspector
    } else {
        RightPane::Hidden
    };

    WorkspaceLayout {
        left,
        center,
        right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default inputs for the common case: repo open, nothing special.
    fn base() -> WorkspaceInputs {
        WorkspaceInputs {
            sidebar_visible: true,
            inspector_visible: true,
            has_detail: true,
            ..Default::default()
        }
    }

    #[test]
    fn default_is_navigator_list_inspector() {
        let l = resolve_workspace(&base());
        assert_eq!(l.left, LeftPane::Navigator);
        assert_eq!(l.center, CenterPane::CommitList);
        assert_eq!(l.right, RightPane::Inspector);
    }

    #[test]
    fn center_precedence_chain() {
        // FileHistory beats everything.
        let i = WorkspaceInputs {
            file_history_open: true,
            ecosystem_open: true,
            loading: true,
            diff_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::FileHistory);
        // Then Ecosystem.
        let i = WorkspaceInputs {
            file_history_open: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Ecosystem);
        // Then Loading.
        let i = WorkspaceInputs {
            ecosystem_open: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Loading);
        // Then Diff.
        let i = WorkspaceInputs {
            loading: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Diff);
    }

    #[test]
    fn takeover_hides_right_but_keeps_sidebar() {
        let i = WorkspaceInputs {
            ecosystem_open: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.left, LeftPane::Navigator);
        assert_eq!(l.right, RightPane::Hidden);
    }

    #[test]
    fn commit_panel_beats_inspector() {
        let i = WorkspaceInputs {
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::CommitPanel);
    }

    #[test]
    fn commit_panel_open_without_entity_hides_right_no_inspector_fallback() {
        let i = WorkspaceInputs {
            commit_panel_open: true,
            commit_panel_present: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn inspector_needs_visible_and_detail() {
        let i = WorkspaceInputs {
            inspector_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
        let i = WorkspaceInputs {
            has_detail: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn sidebar_toggle_hides_left() {
        let i = WorkspaceInputs {
            sidebar_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).left, LeftPane::Hidden);
    }

    // ── T-WS-EDITOR-001: Editor mode precedence ──────────────

    #[test]
    fn editor_mode_shows_file_tree_editor_hunks() {
        let i = WorkspaceInputs {
            editor_mode: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.left, LeftPane::FileTree);
        assert_eq!(l.center, CenterPane::Editor);
        assert_eq!(l.right, RightPane::Hunks);
    }

    #[test]
    fn editor_mode_ignores_open_diff() {
        // Editor mode ignores `main_diff` — center stays Editor, not Diff.
        let i = WorkspaceInputs {
            editor_mode: true,
            diff_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Editor);
    }

    #[test]
    fn file_history_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            file_history_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::FileHistory);
        // Takeover still hides the right panel even in Editor mode.
        assert_eq!(l.right, RightPane::Hidden);
        // Left is independent of the center takeover — still FileTree.
        assert_eq!(l.left, LeftPane::FileTree);
    }

    #[test]
    fn ecosystem_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            ecosystem_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::Ecosystem);
        assert_eq!(l.right, RightPane::Hidden);
        assert_eq!(l.left, LeftPane::FileTree);
    }

    #[test]
    fn loading_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            loading: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Loading);
    }

    #[test]
    fn editor_mode_left_hidden_when_sidebar_toggled_off() {
        let i = WorkspaceInputs {
            editor_mode: true,
            sidebar_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).left, LeftPane::Hidden);
    }

    #[test]
    fn editor_mode_hunks_beats_commit_panel_and_inspector() {
        let i = WorkspaceInputs {
            editor_mode: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hunks);

        let i = WorkspaceInputs {
            editor_mode: true,
            inspector_visible: true,
            has_detail: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hunks);
    }

    #[test]
    fn diff_keeps_right_panel() {
        // T-UI-003 + user request: the right panel stays visible while a diff
        // is open so files can be clicked through continuously.
        let i = WorkspaceInputs {
            diff_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::Diff);
        assert_eq!(l.right, RightPane::Inspector);
    }
}
