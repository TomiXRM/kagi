use super::*;
use gpui::{px, ListState};
use kagi_git::{ChangeKind, CommitId, FileStatus};

fn pr(number: u64, head: &str) -> PullRequest {
    PullRequest {
        number,
        head_sha: head.into(),
        base_repo: "github.com/acme/widgets".into(),
        ..Default::default()
    }
}

fn path() -> PathBuf {
    PathBuf::from("/repo")
}

fn tab(head: &str) -> PrTab {
    PrTab {
        pr: pr(1, head),
        base: CommitId("base".into()),
        base_tip: CommitId("base-tip".into()),
        head: CommitId(head.into()),
        commits: Vec::new(),
        files: vec![FileStatus {
            path: "changed.rs".into(),
            change: ChangeKind::Modified,
        }],
        selected_commit: None,
        selected_file: Some(0),
        diff: None,
        diff_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
        feed_list: ListState::new(0, gpui::ListAlignment::Top, px(400.)),
        feed_entries: std::rc::Rc::new(Vec::new()),
        reviews: Vec::new(),
        comments: Vec::new(),
        line_comments: Vec::new(),
        conversation_loaded: true,
        comment_draft: "keep me".into(),
        conflicts: None,
        conflict_selected: None,
        conflict_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
        conflict_at: 0,
        conflict_preview: None,
        merge_status: None,
        merge_status_loaded: false,
    }
}

#[test]
fn queue_never_starts_more_than_two_or_duplicates_a_stage() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(2, "b"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(3, "c"), PrDetailStage::Status, false, false);
    let first = controller.take_next(Instant::now()).unwrap();
    let second = controller.take_next(Instant::now()).unwrap();
    assert_ne!(first.request.key, second.request.key);
    assert!(controller.take_next(Instant::now()).is_none());
    assert_eq!(controller.active.len(), 2);
}

#[test]
fn an_active_stage_is_not_queued_again_for_the_same_head() {
    let mut controller = PrDetailController::default();
    let pr = pr(1, "a");
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    let started = controller.take_next(Instant::now()).unwrap();
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    assert!(controller.pending.is_empty());

    let ok = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "a".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&started, &ok, Instant::now()), Some(true));
    assert!(controller.take_next(Instant::now()).is_none());
}

#[test]
fn offscreen_pending_rows_are_removed_but_opened_rows_stay() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(2, "b"), PrDetailStage::Status, true, false);
    controller.observe_visible(BTreeSet::new(), BTreeSet::from([2]), 1, Instant::now());
    assert_eq!(controller.pending.len(), 1);
    assert_eq!(controller.pending[0].key.number, 2);
}

#[test]
fn a_failed_refresh_keeps_success_as_stale_and_delays_retry() {
    let mut controller = PrDetailController::default();
    let pr = pr(1, "a");
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    let first = controller.take_next(Instant::now()).unwrap();
    let ok = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "a".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&first, &ok, Instant::now()), Some(true));
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, true);
    let refresh = controller.take_next(Instant::now()).unwrap();
    let failed = Err(kagi_git::github::PrFetchError::Network("offline".into()));
    assert_eq!(
        controller.settle(&refresh, &failed, Instant::now()),
        Some(false)
    );
    assert_eq!(
        controller.availability(1, "github.com/acme/widgets", PrDetailStage::Status, "a"),
        PrDetailAvailability::Stale
    );
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, true);
    assert!(controller.take_next(Instant::now()).is_none());
    assert_eq!(
        controller.availability(1, "github.com/acme/widgets", PrDetailStage::Status, "a"),
        PrDetailAvailability::Stale,
        "retry backoff must not disguise stale data as an active load"
    );
}

#[test]
fn changed_head_invalidates_old_completion() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "old"), PrDetailStage::Status, false, false);
    let old = controller.take_next(Instant::now()).unwrap();
    controller.enqueue(path(), &pr(1, "new"), PrDetailStage::Status, false, false);
    let result = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "old".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&old, &result, Instant::now()), None);
    assert_eq!(
        controller.availability(1, "github.com/acme/widgets", PrDetailStage::Status, "new"),
        PrDetailAvailability::Loading
    );
    assert!(controller.take_next(Instant::now()).is_some());
}

#[test]
fn response_for_a_different_head_never_marks_the_slot_fresh() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "old"), PrDetailStage::Status, false, false);
    let started = controller.take_next(Instant::now()).unwrap();
    let moved = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "new".into(),
        ci: kagi_domain::github::CiState::Success,
        checks: Vec::new(),
        mergeable: kagi_domain::github::Mergeable::Clean,
    }));
    assert_eq!(
        controller.settle(&started, &moved, Instant::now()),
        Some(false)
    );
    assert_eq!(
        controller.availability(1, "github.com/acme/widgets", PrDetailStage::Status, "old"),
        PrDetailAvailability::Missing
    );
}

#[test]
fn list_head_change_invalidates_then_local_reload_restores_the_open_tab() {
    let mut tab = tab("old");
    let mut listed = pr(1, "new");
    listed.title = "new title".into();
    sync_open_pr_from_list(&mut tab, &listed);
    assert_eq!(tab.pr.title, "new title");
    assert_eq!(tab.pr.head_sha, "new");
    assert_eq!(
        tab.head,
        CommitId("old".into()),
        "old local head triggers reload"
    );
    assert!(tab.files.is_empty());
    assert_eq!(tab.selected_file, None);
    assert_eq!(tab.comment_draft, "keep me");
    assert!(tab.conversation_loaded);

    install_local_pr_head(
        &mut tab,
        CommitId("new-base".into()),
        CommitId("new-base-tip".into()),
        CommitId("new".into()),
        Vec::new(),
        vec![FileStatus {
            path: "new.rs".into(),
            change: ChangeKind::Added,
        }],
    );
    assert_eq!(tab.head, CommitId("new".into()));
    assert_eq!(tab.files[0].path, std::path::PathBuf::from("new.rs"));
    assert_eq!(tab.selected_file, Some(0));
}

#[test]
fn one_apply_function_updates_list_and_open_copies_only_for_the_same_head() {
    let mut list = vec![pr(1, "same"), pr(2, "other")];
    let mut opened = vec![pr(1, "same"), pr(1, "moved")];
    let detail = PrStatusDetail {
        number: 1,
        head_sha: "same".into(),
        ci: kagi_domain::github::CiState::Success,
        checks: Vec::new(),
        mergeable: kagi_domain::github::Mergeable::Clean,
    };
    apply_status_copies(&mut list, opened.iter_mut(), &detail);
    assert_eq!(list[0].ci, kagi_domain::github::CiState::Success);
    assert_eq!(opened[0].ci, kagi_domain::github::CiState::Success);
    assert_eq!(list[1].ci, kagi_domain::github::CiState::None);
    assert_eq!(opened[1].ci, kagi_domain::github::CiState::None);
}
