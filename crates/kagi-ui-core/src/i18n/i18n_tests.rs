use super::*;
use parking_lot::Mutex;

// The active-language atomic is process-global; serialise the tests that
// mutate it so they don't race.
pub(super) static LOCK: Mutex<()> = Mutex::new(());

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
fn domain_words_stay_english_in_both_langs() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::Ja);
    // The toolbar guards keep the domain word "Pull" verbatim.
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
    assert!(wip_row_note(2).contains("クリックで commit panel"));
    assert_eq!(
        graph_detached_worktree_label("abc12345"),
        "detached abc12345"
    );
    set_lang_no_persist(Lang::En);
}
#[test]
fn issues_messages_and_comment_count_switch() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    assert_eq!(Msg::IssuesEmpty.t(), "No open issues");
    assert_eq!(issue_comments(2), "Comments (2)");
    set_lang_no_persist(Lang::Ja);
    assert_eq!(Msg::IssuesEmpty.t(), "open Issues はありません");
    assert_eq!(issue_comments(2), "コメント (2)");
    set_lang_no_persist(Lang::En);
}

#[test]
fn op_failed_switches_and_keeps_domain_words() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    assert_eq!(op_failed(Op::Pull, "boom"), "Pull failed: boom");
    assert_eq!(op_failed(Op::RepoOpen, "boom"), "Repo open failed: boom");
    assert_eq!(op_plan_failed(Op::Push, "boom"), "Push plan failed: boom");
    set_lang_no_persist(Lang::Ja);
    assert_eq!(
        op_failed(Op::Pull, "boom"),
        "pull \u{306b}\u{5931}\u{6557}\u{3057}\u{307e}\u{3057}\u{305f}: boom"
    );
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

// #353: the equivalent-command line must read "equivalent to" / "相当",
// NEVER "runs" / "実行" — X, so
// claiming it "runs" the command would be a lie.
#[test]
fn equivalent_command_wording_says_equivalent_not_runs() {
    let _g = LOCK.lock();
    set_lang_no_persist(Lang::En);
    let en = Msg::PlanEquivalentTo.t();
    assert!(en.contains("equivalent to"), "EN wording: {en}");
    assert!(!en.contains("runs"), "EN must not say 'runs': {en}");
    assert!(en.contains("{}"), "EN keeps the command placeholder: {en}");

    set_lang_no_persist(Lang::Ja);
    let ja = Msg::PlanEquivalentTo.t();
    assert!(ja.contains("相当"), "JA wording: {ja}");
    assert!(!ja.contains("実行"), "JA must not say '実行': {ja}");

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
    assert_eq!(
        branch_name_error(&B::EmptyCreate),
        "Branch name must not be empty."
    );
    assert_eq!(
        worktree_path_error(&W::Empty),
        "Worktree path must not be empty."
    );
    set_lang_no_persist(Lang::Ja);
    // Localized — no longer the English sentence, and the name stays verbatim.
    assert_ne!(
        branch_name_error(&B::EmptyCreate),
        "Branch name must not be empty."
    );
    assert!(branch_name_error(&B::CreateExists("feat".into())).contains("feat"));
    assert!(worktree_path_error(&W::Exists("/p".into())).contains("/p"));
    set_lang_no_persist(Lang::En);
}
