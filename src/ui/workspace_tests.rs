use super::*;
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

    /// R-STASH-PEEK: a compare opened with NO commit selected (stash peek)
    /// must still get the right pane — it does not depend on commit detail.
    #[test]
    fn compare_shows_without_a_selected_commit() {
        let layout = resolve_workspace(&WorkspaceInputs {
            compare_open: true,
            has_detail: false,
            ..base()
        });
        assert_eq!(layout.right, RightPane::Compare);
    }

    #[test]
    fn default_is_navigator_list_inspector() {
        let l = resolve_workspace(&base());
        assert_eq!(l.left, LeftPane::Navigator);
        assert_eq!(l.center, CenterPane::CommitList);
        assert_eq!(l.right, RightPane::Inspector);
    }

    #[test]
    fn issues_mode_uses_sidebar_and_main_takeover_without_outer_right_pane() {
        let layout = resolve_workspace(&WorkspaceInputs {
            issues_mode: true,
            ..base()
        });
        assert_eq!(layout.left, LeftPane::IssueList);
        assert_eq!(layout.center, CenterPane::IssuesMode);
        assert_eq!(layout.right, RightPane::Hidden);
    }

    #[test]
    fn prs_precede_issues_if_a_caller_breaks_mode_exclusivity() {
        let layout = resolve_workspace(&WorkspaceInputs {
            pr_mode: true,
            issues_mode: true,
            ..base()
        });
        assert_eq!(layout.center, CenterPane::PrMode);
    }

    /// The contract `KagiApp::leave_takeovers` encodes. Each mode button has to
    /// close everything that outranks the pane it is selecting, or the click
    /// changes nothing on screen while the toolbar lights that button up as the
    /// active mode (user report: Graph/PRs/Editor were all dead while Branch
    /// Cleanup was open). If this precedence changes, that list has to change
    /// with it — this test is what makes that fail loudly.
    #[test]
    fn a_mode_is_only_visible_once_everything_above_it_is_closed() {
        let open_all = WorkspaceInputs {
            file_history_open: true,
            ecosystem_open: true,
            branch_cleanup_open: true,
            pr_mode: true,
            editor_mode: true,
            ..base()
        };
        // PR mode is invisible until all three takeovers above it are closed.
        assert_eq!(resolve_workspace(&open_all).center, CenterPane::FileHistory);
        let i = WorkspaceInputs {
            file_history_open: false,
            ecosystem_open: false,
            ..open_all
        };
        assert_eq!(
            resolve_workspace(&i).center,
            CenterPane::BranchCleanup,
            "Branch Cleanup outranks PR mode — the one the mode buttons forgot"
        );
        let i = WorkspaceInputs {
            branch_cleanup_open: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::PrMode);
        // And Editor needs PR mode closed on top of that.
        let i = WorkspaceInputs {
            pr_mode: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Editor);
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

    // ── ADR-0121 B2: Compare precedence ──────────────────────

    #[test]
    fn compare_replaces_inspector_with_same_gates() {
        // Compare wins over the plain Inspector...
        let i = WorkspaceInputs {
            compare_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Compare);
        // ...and hides only with the inspector toggle. `has_detail` is NOT a
        // gate for compare any more (R-STASH-PEEK: a stash peek opens a
        // compare with no commit selected).
        let i = WorkspaceInputs {
            inspector_visible: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
        let i = WorkspaceInputs {
            inspector_visible: true,
            has_detail: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Compare);
    }

    #[test]
    fn commit_panel_beats_compare() {
        let i = WorkspaceInputs {
            compare_open: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::CommitPanel);
    }

    #[test]
    fn takeover_hides_compare() {
        let i = WorkspaceInputs {
            compare_open: true,
            ecosystem_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
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
