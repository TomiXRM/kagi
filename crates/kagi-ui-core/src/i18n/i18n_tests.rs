use super::*;
use parking_lot::Mutex;

// The active-language atomic is process-global; serialise the tests that
// mutate it so they don't race.
pub(super) static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn diff_line_ending_annotations_are_localized_in_both_languages() {
    for (message, english, japanese) in [
        (Msg::DiffLineEndingLf, "LF line ending", "LF 改行"),
        (Msg::DiffLineEndingCrLf, "CRLF line ending", "CRLF 改行"),
        (Msg::DiffLineEndingNone, "No line ending", "改行なし"),
    ] {
        assert_eq!(message.t_for(Lang::En), english);
        assert_eq!(message.t_for(Lang::Ja), japanese);
    }
}

#[test]
fn shortcuts_remap_help_explains_the_settings_procedure() {
    let english = Msg::ShortcutsRemapHelp.t_for(Lang::En);
    let japanese = Msg::ShortcutsRemapHelp.t_for(Lang::Ja);
    assert_ne!(english, japanese);
    for text in [english, japanese] {
        for syntax in [
            "~/.kagi/settings.json",
            "keybinding.<command id>",
            "keybinding.view.toggleSidebar",
            "secondary-shift-b",
            "\"\"",
        ] {
            assert!(text.contains(syntax), "{text}");
        }
    }
    assert!(english.contains("restart"));
    assert!(japanese.contains("再起動"));
}

#[test]
fn palette_descriptions_use_localized_sentence_boundaries() {
    assert_eq!(
        command_palette_description(Lang::En, "Actions", Some("Unavailable")),
        "Actions. Unavailable"
    );
    assert_eq!(
        command_palette_description(Lang::Ja, "操作", Some("実行できません")),
        "操作。実行できません"
    );
    assert_eq!(
        command_palette_description(Lang::En, "Appearance", None),
        "Appearance"
    );
    assert_eq!(command_palette_description(Lang::Ja, "外観", None), "外観");
}

#[test]
fn editor_find_controls_have_semantic_names_in_both_languages() {
    for (message, english, japanese) in [
        (Msg::EditorFindPrevious, "Previous match", "前の一致"),
        (Msg::EditorFindNext, "Next match", "次の一致"),
        (Msg::EditorFindReplace, "Replace", "置換"),
        (Msg::EditorFindReplaceAll, "Replace all", "すべて置換"),
        (Msg::EditorFindClose, "Close", "閉じる"),
        (Msg::EditorFindCase, "Match case", "大文字と小文字を区別"),
        (
            Msg::EditorFindReplaceMode,
            "Show replacement field",
            "置換欄を表示",
        ),
        (Msg::EditorFindField, "Find", "検索"),
        (Msg::EditorFindReplacementField, "Replacement", "置換文字列"),
    ] {
        assert_eq!(message.t_for(Lang::En), english);
        assert_eq!(message.t_for(Lang::Ja), japanese);
    }
}

#[test]
fn terminal_empty_states_are_localized_and_keep_os_detail() {
    for (message, english, japanese) in [
        (
            Msg::TerminalNotStartedTitle,
            "Shell not started",
            "shell は未起動です",
        ),
        (
            Msg::TerminalExitedTitle,
            "Shell exited",
            "shell が終了しました",
        ),
        (
            Msg::TerminalFailedTitle,
            "Shell failed",
            "shell でエラーが発生しました",
        ),
        (
            Msg::TerminalNotStartedHint,
            "Start a shell in this repository.",
            "このリポジトリで shell を起動します。",
        ),
        (
            Msg::TerminalRestartHint,
            "Restart to open a new shell in this repository.",
            "再起動すると、このリポジトリで新しい shell が開きます。",
        ),
        (Msg::TerminalRestart, "Restart", "Restart"),
    ] {
        assert_eq!(message.t_for(Lang::En), english);
        assert_eq!(message.t_for(Lang::Ja), japanese);
    }
    let _lock = LOCK.lock();
    let old = ACTIVE.load(Ordering::Relaxed);
    for (language, expected) in [
        (Lang::En, "Shell exited (code 3)"),
        (Lang::Ja, "shell が終了しました（終了コード 3）"),
    ] {
        ACTIVE.store(language.index(), Ordering::Relaxed);
        assert_eq!(terminal_exited_title(3), expected);
    }
    ACTIVE.store(old, Ordering::Relaxed);
    let reason = "spawn '/missing/{shell}': No such file or directory (os error 2)";
    assert!(terminal_failed_hint(reason).ends_with(reason));
}

#[test]
fn slug_roundtrip() {
    assert_eq!(Lang::from_slug("en"), Some(Lang::En));
    assert_eq!(Lang::from_slug("JA"), Some(Lang::Ja));
    assert_eq!(Lang::from_slug(" ja "), Some(Lang::Ja));
    assert_eq!(Lang::from_slug("fr"), None);
    assert_eq!(Lang::En.slug(), "en");
    assert_eq!(Lang::Ja.slug(), "ja");
}

#[test]
fn copy_all_equivalent_command_heading_is_localized() {
    assert_eq!(
        Msg::ModalEquivalentCommand.t_for(Lang::En),
        "equivalent command:"
    );
    assert_eq!(
        Msg::ModalEquivalentCommand.t_for(Lang::Ja),
        "相当するコマンド:"
    );
}

#[test]
fn compact_composer_localizes_prose_and_keeps_git_controls_english() {
    assert_eq!(Msg::BottomTabChanges.t_for(Lang::En), "Changes");
    assert_eq!(Msg::BottomTabChanges.t_for(Lang::Ja), "変更");
    assert_eq!(
        Msg::CompactCommitComposer.t_for(Lang::Ja),
        "Commit メッセージ"
    );
    assert_eq!(
        Msg::CompactCommitClose.t_for(Lang::Ja),
        "Commit の入力を閉じる"
    );
    assert_eq!(
        Msg::CommitPanelNothingUnstaged.t_for(Lang::En),
        "No unstaged changes"
    );
    assert_eq!(
        Msg::CommitPanelNothingUnstaged.t_for(Lang::Ja),
        "未 stage の変更がありません"
    );
    assert_eq!(Msg::CompactCommitAffordance.t_for(Lang::En), "Commit…");
    assert_eq!(Msg::CompactCommitAffordance.t_for(Lang::Ja), "Commit…");
}

#[test]
fn commit_panel_chrome_has_japanese_without_english_fallbacks() {
    for (message, japanese) in [
        (Msg::CommitPanelTitle, "Commit Panel"),
        (Msg::CommitPanelUnstaged, "未 stage"),
        (Msg::CommitPanelStaged, "stage 済み"),
        (Msg::CommitPanelList, "一覧"),
        (Msg::CommitPanelTree, "ツリー"),
        (Msg::CommitPanelStageAll, "Stage all"),
        (Msg::CommitPanelUnstageAll, "Unstage all"),
        (Msg::CommitPanelDiscardAll, "Discard all"),
        (Msg::CommitPanelStage, "Stage"),
        (Msg::CommitPanelUnstage, "Unstage"),
        (Msg::CommitPanelConflict, "conflict"),
    ] {
        assert_eq!(message.t_for(Lang::Ja), japanese);
        if matches!(
            message,
            Msg::CommitPanelUnstaged
                | Msg::CommitPanelStaged
                | Msg::CommitPanelList
                | Msg::CommitPanelTree
                | Msg::CommitPanelConflict
        ) {
            assert_ne!(message.t_for(Lang::En), japanese);
        } else {
            assert_eq!(message.t_for(Lang::En), japanese);
        }
    }
}

#[test]
fn toolbar_unavailable_reasons_are_japanese_prose() {
    for message in [
        Msg::PullBusy,
        Msg::PullDetached,
        Msg::PullUnborn,
        Msg::PullNoUpstream,
        Msg::PullNothing,
        Msg::PushBusy,
        Msg::PushDetached,
        Msg::PushUnborn,
        Msg::PushNoRemote,
        Msg::PushNothing,
        Msg::StashClean,
        Msg::StashBusy,
        Msg::PopEmpty,
        Msg::PopBusy,
        Msg::NothingToUndo,
        Msg::NothingToRedo,
        Msg::NoRepoOpen,
    ] {
        let japanese = message.t_for(Lang::Ja);
        for fragment in [
            "nothing to",
            "is clean",
            "configured",
            "is empty",
            "in progress",
            "switch to",
            "no commits",
        ] {
            assert!(!japanese.contains(fragment), "{message:?}: {japanese}");
        }
        assert_ne!(japanese, message.t_for(Lang::En));
    }
}

#[test]
fn t_switches_with_set_lang() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    assert_eq!(Msg::OpInProgress.t(), "another operation is in progress");
    assert_eq!(Msg::BusyCheckout.t(), "checkout in progress…");
    set_lang_no_persist(Lang::Ja);
    assert_eq!(Msg::OpInProgress.t(), "別の操作が実行中です");
    assert_eq!(Msg::BusyCheckout.t(), "checkout 実行中…");
    set_lang_no_persist(Lang::En);
}

#[test]
fn conflict_repository_busy_names_the_other_window_in_both_languages() {
    assert_eq!(
        Msg::AdviceConflictsRepositoryBusy.t_for(Lang::En),
        "Another Kagi window is operating on this repository"
    );
    assert_eq!(
        Msg::AdviceConflictsRepositoryBusy.t_for(Lang::Ja),
        "別の Kagi ウィンドウがこの repository を操作中です"
    );
}

#[test]
fn domain_words_stay_english_in_both_langs() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::Ja);
    assert!(Msg::PullDetached.t().starts_with("Pull:"));
    assert!(Msg::PushDetached.t().starts_with("Push:"));
    // ADR-0048: conflict-domain words stay English even in Japanese
    // (conflict / merge / resolved / unresolved are never translated).
    assert_eq!(Msg::EditorConflictNofM.t(), "conflict");
    assert_eq!(Msg::ConflictResolved.t(), "resolved");
    assert_eq!(Msg::ConflictResolvedShort.t(), "resolved");
    assert_eq!(Msg::ConflictUnresolved.t(), "unresolved");
    assert_eq!(Msg::ConflictConflictedCount.t(), "conflicted");
    assert_eq!(Msg::ConflictResolvedCount.t(), "resolved");
    for m in [
        Msg::ConflictSelectFile,
        Msg::ConflictCopyPath,
        Msg::ConflictSectionConflicted,
        Msg::ConflictSectionResolved,
        Msg::MergeAndResolveConflicts,
        Msg::MergeConflictWarning,
        Msg::EditorNoTextMerge,
    ] {
        assert!(!m.t().contains('衝'), "{:?} still contains 衝突", m);
        assert!(!m.t().contains("マージ"), "{:?} still contains マージ", m);
    }
    set_lang_no_persist(Lang::En);
}

#[test]
fn japanese_git_terms_preserve_operation_names_and_working_tree_meaning() {
    for (message, english, japanese) in [
        (Msg::ToolbarBranch, "Branch", "Branch"),
        (Msg::ToolbarStash, "Stash", "Stash"),
        (Msg::ToolbarPop, "Pop", "Pop"),
        (Msg::InputStash, "Stash", "Stash"),
        (Msg::InputStashTitle, "Stash changes", "変更を stash する"),
        (Msg::A11ySidebarWorktrees, "Worktrees", "worktree"),
        (Msg::A11ySidebarStashes, "Stashes", "stash"),
        (Msg::PrWhyConflicting, "Conflicts", "conflict"),
        (
            Msg::StashClean,
            "Stash: working tree is clean — nothing to stash",
            "working tree に stash する変更がありません",
        ),
        (
            Msg::PopEmpty,
            "Pop: stash is empty",
            "stash した変更がありません",
        ),
        (
            Msg::EditorWorkspaceEmpty,
            "No changed files in the working tree.",
            "working tree に変更されたファイルはありません。",
        ),
    ] {
        assert_eq!(message.t_for(Lang::En), english);
        assert_eq!(message.t_for(Lang::Ja), japanese);
    }
    assert_eq!(
        Msg::StashPopConflictedKept.t_for(Lang::Ja),
        "conflict ありで適用しました — stash は保持されています。conflict を解決してから手動で drop してください。",
    );
    assert!(plan::rebase::ADVICE_REBASE_DIRTY_WORKING_TREE.starts_with("working tree "));
    assert!(plan::worktree::ADVICE_WORKTREE_CREATES_LINKED_WORKTREE.contains("worktree"));
    assert!(plan::branch::ADVICE_BRANCH_DELETE_SQUASH_MERGED.contains("merge 先に反映済み"));
}

#[test]
fn parameterized_helpers_switch() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    assert_eq!(
        wip_row_note(1),
        "// WIP — 1 change (click to open commit panel)"
    );
    assert_eq!(
        wip_row_note(3),
        "// WIP — 3 changes (click to open commit panel)"
    );
    assert_eq!(
        graph_detached_worktree_label("abc12345"),
        "detached abc12345"
    );
    set_lang_no_persist(Lang::Ja);
    assert!(wip_row_note(2).contains("クリックで Commit Panel"));
    assert_eq!(
        graph_detached_worktree_label("abc12345"),
        "detached abc12345"
    );
    set_lang_no_persist(Lang::En);
}
#[test]
fn op_failed_switches_and_keeps_domain_words() {
    let _g = LOCK.lock();
    for language in [Lang::En, Lang::Ja] {
        set_lang_no_persist(language);
        let raw = "raw Git detail: branch feature_session-name";
        assert!(op_failed(Op::Pull, raw).contains(raw));
        assert!(op_plan_failed(Op::Push, raw).contains(raw));
        assert!(!op_plan_failed(Op::Push, raw).contains("plan"));
    }
    set_lang_no_persist(Lang::Ja);
    // ADR-0048: git domain words stay English in the Japanese label too.
    for op in [
        Op::Pull,
        Op::Push,
        Op::Merge,
        Op::Commit,
        Op::Rebase,
        Op::Stash,
    ] {
        assert!(
            op.t().is_ascii(),
            "{:?} japanese label must keep the domain word in English",
            op
        );
    }
    set_lang_no_persist(Lang::En);
}

#[test]
fn resolve_lang_env_override() {
    let _g = LOCK.lock();
    // KAGI_LANG takes top priority and is deterministic for headless tests.
    std::env::set_var("KAGI_LANG", "ja");
    assert_eq!(resolve_lang(), Lang::Ja);
    std::env::set_var("KAGI_LANG", "en");
    assert_eq!(resolve_lang(), Lang::En);
    std::env::remove_var("KAGI_LANG");
}

// Wording lock for the GIT-LAYER test fixtures: those tests pin the exact
// English `Display` text of these errors. It is NOT a UI guarantee - the UI
// never calls `Display`, it goes through `branch_name_error` (covered by
// `keyed_validation_localizes` below). Three representative variants:
// unit, one-arg, and the multi-line one.
#[test]
fn keyed_validation_display_is_exact_english() {
    use kagi_domain::plan::{BranchNameError as B, WorktreePathError as W};
    assert_eq!(B::EmptyCreate.to_string(), "Branch name must not be empty.");
    assert_eq!(
        B::CreateInvalidRef("x y".into()).to_string(),
        "Branch name 'x y' is not a valid git ref name \
             (no spaces, '..', or other invalid characters)."
    );
    assert_eq!(
        W::Exists("/p".into()).to_string(),
        "Worktree path '/p' already exists."
    );
}

#[test]
fn keyed_validation_localizes() {
    use kagi_domain::plan::{BranchNameError as B, WorktreePathError as W};
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    assert_eq!(branch_name_error(&B::EmptyCreate), "Enter a branch name.");
    assert_eq!(
        worktree_path_error(&W::Empty),
        "Worktree path must not be empty."
    );
    set_lang_no_persist(Lang::Ja);
    // Localized — no longer the English sentence, and the name stays verbatim.
    assert_ne!(branch_name_error(&B::EmptyCreate), "Enter a branch name.");
    assert!(branch_name_error(&B::CreateExists("feat".into())).contains("feat"));
    assert!(worktree_path_error(&W::Exists("/p".into())).contains("/p"));
    set_lang_no_persist(Lang::En);
}

#[test]
fn advice_keeps_identifiers_literal_and_their_roles_distinct() {
    use kagi_domain::plan_note::{BranchNote, SwitchNote};

    let branch = "feature/{}-レビュー";
    let remote = "origin/{}-追跡";
    let tip = "abc123456789";
    let deletion = plan::branch::note_ja(&BranchNote::DeleteUnmerged {
        name: branch.into(),
        tip: tip.into(),
        commits: 17,
    });
    assert!(deletion.contains(&format!("branch `{branch}` / 先端 `{tip}`")));
    assert!(deletion.contains("17 commit"));

    let switch = plan::switch::note_ja(&SwitchNote::DivergedSwitchOnly {
        name: branch.into(),
        remote: remote.into(),
        ahead: 3,
        behind: 7,
    });
    assert!(switch.starts_with(&format!("{remote} から分岐")));
    assert!(switch.contains("3 commit 進み、7 commit 遅れ"));
    assert!(switch.ends_with(&format!("branch `{branch}`")));
}

#[test]
fn zero_loss_branch_deletion_describes_retention_without_second_confirmation() {
    use kagi_domain::plan_note::{BranchNote, PlanNote};
    let _guard = LOCK.lock();
    let note = PlanNote::Branch(BranchNote::DeleteUnmerged {
        name: "feature/{}-レビュー".into(),
        tip: "abc123456789".into(),
        commits: 0,
    });
    for language in [Lang::En, Lang::Ja] {
        set_lang_no_persist(language);
        let text = plan_note_text(&note);
        assert!(text.contains("feature/{}-レビュー") && text.contains("abc123456789"));
        assert!(!text.contains("Confirm twice") && !text.contains("2 回確認"));
        assert!(
            !text.contains("0 commits unreachable")
                && !text.contains("0 commit が他の ref から到達不能")
        );
        assert!(text.contains(if language == Lang::En {
            "recovery ref"
        } else {
            "復元用 ref"
        }));
    }
    set_lang_no_persist(Lang::En);
}

#[test]
fn explicit_japanese_advice_preserves_stash_syntax_under_english_locale() {
    use kagi_domain::plan_note::CommonNote;

    let _guard = LOCK.lock();
    set_lang_no_persist(Lang::En);
    let advice = plan::common::note_ja(&CommonNote::DirtyStashFirst);
    assert!(advice.contains("先に変更を stash"));
    assert!(advice.contains("stash@{0}"));
    assert!(advice.contains("`git stash pop`"));
}

#[test]
fn stash_identity_advice_preserves_git_braces_and_identifier_data() {
    use kagi_domain::plan_note::{PlanNote, StashNote};

    let _guard = LOCK.lock();
    let expected = "approved-{}-先端";
    let note = PlanNote::Stash(StashNote::TargetChanged {
        index: 3,
        expected: expected.into(),
    });
    for language in [Lang::En, Lang::Ja] {
        set_lang_no_persist(language);
        let text = plan_note_text(&note);
        assert!(text.contains("stash@{3}"), "{language:?}: {text}");
        assert!(text.contains(expected), "{language:?}: {text}");
    }
}

#[test]
fn special_repo_blockers_name_variants_in_english_and_japanese() {
    use kagi_domain::plan_note::{CommonNote, PlanNote, SparseCheckoutKind};

    let _guard = LOCK.lock();
    let previous = lang();
    let path = "module-{}-日本語";
    for language in [Lang::En, Lang::Ja] {
        set_lang_no_persist(language);
        for kind in [
            SparseCheckoutKind::Cone,
            SparseCheckoutKind::NonCone,
            SparseCheckoutKind::SkipWorktree,
        ] {
            let text = plan_note_text(&PlanNote::Common(CommonNote::SparseCheckoutUnsupported {
                kind,
            }));
            let expected = match kind {
                SparseCheckoutKind::Cone => "cone sparse-checkout",
                SparseCheckoutKind::NonCone => "non-cone sparse-checkout",
                SparseCheckoutKind::SkipWorktree if language == Lang::En => "skip-worktree entries",
                SparseCheckoutKind::SkipWorktree => "skip-worktree エントリー",
            };
            assert!(text.contains(expected), "{text}");
        }
        let filter_text = plan_note_text(&PlanNote::Common(CommonNote::ExternalFilterSync {
            path: path.into(),
            filter: "lfs".into(),
        }));
        assert!(
            filter_text.contains(path) && filter_text.contains("(lfs)"),
            "{filter_text}"
        );
        assert!(
            !filter_text.contains("stage") && !filter_text.contains("discard"),
            "{filter_text}"
        );
        for uninitialized in [true, false] {
            let text = plan_note_text(&PlanNote::Common(
                CommonNote::SubmoduleCheckoutUnsupported {
                    path: path.into(),
                    uninitialized,
                },
            ));
            assert!(text.contains(path), "{text}");
            let other = plan_note_text(&PlanNote::Common(
                CommonNote::SubmoduleCheckoutUnsupported {
                    path: path.into(),
                    uninitialized: !uninitialized,
                },
            ));
            assert_ne!(
                text, other,
                "distinct submodule states need distinct guidance"
            );
        }
    }
    set_lang_no_persist(previous);
}

/// #934: both ignored-content shapes fill every JA slot (a slot mismatch
/// panics in `advice_text`) and keep the counts and path the user confirms.
#[test]
fn ignored_files_remove_warning_renders_counts_in_japanese() {
    use kagi_domain::plan_note::{PlanNote, WorktreeNote};

    let _guard = LOCK.lock();
    set_lang_no_persist(Lang::Ja);
    let files_only = plan_note_text(&PlanNote::Worktree(WorktreeNote::RemoveIgnoredFiles {
        path: "/wt/a".into(),
        files: 4,
        folders: 0,
    }));
    assert_eq!(
        files_only,
        "Git が追跡しない ignored ファイルが 4 件あります。削除すると完全に消えます。\nworktree `/wt/a`"
    );
    let with_folders = plan_note_text(&PlanNote::Worktree(WorktreeNote::RemoveIgnoredFiles {
        path: "/wt/a".into(),
        files: 2,
        folders: 1,
    }));
    assert_eq!(
        with_folders,
        "Git が追跡しない ignored ファイル 2 件と ignored フォルダ 1 件(中身は数えていません)があります。削除すると完全に消えます。\nworktree `/wt/a`"
    );
    set_lang_no_persist(Lang::En);
}

#[test]
fn native_window_menu_and_about_labels() {
    for (msg, en, ja) in [
        (Msg::AboutTitle, "About Kagi", "Kagiについて"),
        (Msg::MenuHideSidebar, "Hide Sidebar", "サイドバーを非表示"),
        (Msg::MenuShowSidebar, "Show Sidebar", "サイドバーを表示"),
        (
            Msg::MenuEnterFullScreen,
            "Enter Full Screen",
            "フルスクリーンにする",
        ),
        (
            Msg::MenuExitFullScreen,
            "Exit Full Screen",
            "フルスクリーンを解除",
        ),
        (Msg::MenuPreviousTab, "Show Previous Tab", "前のタブを表示"),
        (Msg::MenuNextTab, "Show Next Tab", "次のタブを表示"),
        (
            Msg::MenuBringAllToFront,
            "Bring All to Front",
            "すべてを手前に移動",
        ),
    ] {
        assert_eq!(msg.t_for(Lang::En), en);
        assert_eq!(msg.t_for(Lang::Ja), ja);
    }
}

#[test]
fn chooser_row_names_preserve_external_identity_in_both_languages() {
    let _lock = LOCK.lock();
    let old = ACTIVE.load(Ordering::Relaxed);
    for (language, issue, recent) in [
        (
            Lang::En,
            "Open issue #42, Fix {} {state} 日本語, Open",
            "Open repository repo {} 日本語",
        ),
        (
            Lang::Ja,
            "Issue #42 を開く、Fix {} {state} 日本語、Open",
            "リポジトリ repo {} 日本語 を開く",
        ),
    ] {
        ACTIVE.store(language.index(), Ordering::Relaxed);
        assert_eq!(issue_row_name(42, "Fix {} {state} 日本語", "Open"), issue);
        assert_eq!(recent_row_name("repo {} 日本語"), recent);
        for message in [
            Msg::BranchPickerCheckout,
            Msg::BranchPickerDelete,
            Msg::BranchPickerRename,
        ] {
            assert!(!message.t().is_empty());
        }
    }
    ACTIVE.store(old, Ordering::Relaxed);
}

#[test]
fn sidebar_filtered_disclosure_reason_is_localized() {
    assert_eq!(
        Msg::SidebarGroupFilterExpanded.t_for(Lang::En),
        "Groups stay expanded while filtering. Clear the filter to collapse a group."
    );
    assert_eq!(
        Msg::SidebarGroupFilterExpanded.t_for(Lang::Ja),
        "絞り込み中はグループを展開したままにします。グループを折りたたむには絞り込みを解除してください。"
    );
}
