//! Session-owned lazy PR detail reads (T-PR-LAZY-FETCH).
//!
//! The controller stores scheduling state only. Fetched values continue to
//! live in the composed `PullRequest` values owned by `TabUiState` and its
//! open `PrTab`s. Every completion carries the owner and repository identity
//! captured when it started, so a tab switch cannot redirect it.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::Context;
use kagi_domain::github::{PrBodyDetail, PrDetailAvailability, PrStatusDetail, PullRequest};

use super::KagiApp;

const DETAIL_CONCURRENCY: usize = 2;
const VISIBLE_DEBOUNCE: Duration = Duration::from_millis(200);
const VISIBLE_MAX_WAIT: Duration = Duration::from_millis(500);
const RETRY_DELAY: Duration = Duration::from_secs(15);
const STATUS_FRESH_FOR: Duration = Duration::from_secs(60);
const BODY_FRESH_FOR: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PrDetailStage {
    Status,
    Body,
}

impl PrDetailStage {
    fn fresh_for(self) -> Duration {
        match self {
            Self::Status => STATUS_FRESH_FOR,
            Self::Body => BODY_FRESH_FOR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DetailKey {
    base_repo: String,
    number: u64,
    stage: PrDetailStage,
}

#[derive(Debug, Clone)]
struct QueuedDetail {
    key: DetailKey,
    repo_path: PathBuf,
    base_repo: String,
    head_sha: String,
    priority: bool,
}

#[derive(Debug, Clone)]
struct StartedDetail {
    request: QueuedDetail,
    generation: u64,
}

#[derive(Debug, Default)]
struct DetailSlot {
    generation: u64,
    head_sha: String,
    availability: PrDetailAvailability,
    refreshed_at: Option<Instant>,
    successful_head: Option<String>,
    next_retry_at: Option<Instant>,
    error: Option<String>,
}

/// Scheduling and freshness state for one repository tab.
#[derive(Debug, Default)]
pub(super) struct PrDetailController {
    visible: BTreeSet<u64>,
    opened: BTreeSet<u64>,
    visible_generation: u64,
    visible_epoch: u64,
    visible_burst_started: Option<Instant>,
    pending: VecDeque<QueuedDetail>,
    active: HashSet<(DetailKey, u64)>,
    slots: HashMap<DetailKey, DetailSlot>,
}

impl PrDetailController {
    pub(super) fn availability(
        &self,
        number: u64,
        base_repo: &str,
        stage: PrDetailStage,
        head_sha: &str,
    ) -> PrDetailAvailability {
        let Some(slot) = self.slots.get(&DetailKey {
            base_repo: base_repo.to_string(),
            number,
            stage,
        }) else {
            return PrDetailAvailability::Missing;
        };
        if slot.head_sha != head_sha {
            return PrDetailAvailability::Missing;
        }
        if slot.availability == PrDetailAvailability::Fresh
            && slot
                .refreshed_at
                .is_some_and(|at| at.elapsed() >= stage.fresh_for())
        {
            return PrDetailAvailability::Stale;
        }
        slot.availability
    }

    fn observe_visible(
        &mut self,
        visible: BTreeSet<u64>,
        opened: BTreeSet<u64>,
        epoch: u64,
        now: Instant,
    ) -> Option<(u64, Duration)> {
        if self.visible == visible && self.opened == opened && self.visible_epoch == epoch {
            return None;
        }
        self.visible = visible;
        self.opened = opened;
        self.visible_epoch = epoch;
        self.pending.retain(|request| {
            request.priority
                || self.opened.contains(&request.key.number)
                || (request.key.stage == PrDetailStage::Status
                    && self.visible.contains(&request.key.number))
        });
        self.visible_generation = self.visible_generation.wrapping_add(1);
        let started = *self.visible_burst_started.get_or_insert(now);
        let remaining = VISIBLE_MAX_WAIT.saturating_sub(now.duration_since(started));
        Some((self.visible_generation, VISIBLE_DEBOUNCE.min(remaining)))
    }

    fn finish_visible_debounce(&mut self, generation: u64, epoch: u64) -> Option<BTreeSet<u64>> {
        if generation != self.visible_generation || epoch != self.visible_epoch {
            return None;
        }
        self.visible_burst_started = None;
        Some(self.visible.clone())
    }

    fn leave_home(&mut self, opened: BTreeSet<u64>) {
        self.opened = opened;
        self.visible.clear();
        self.visible_generation = self.visible_generation.wrapping_add(1);
        self.visible_burst_started = None;
        self.pending
            .retain(|request| request.priority || self.opened.contains(&request.key.number));
    }

    fn enqueue(
        &mut self,
        repo_path: PathBuf,
        pr: &PullRequest,
        stage: PrDetailStage,
        priority: bool,
        force: bool,
    ) {
        let key = DetailKey {
            base_repo: pr.base_repo.clone(),
            number: pr.number,
            stage,
        };
        if !force
            && self.availability(pr.number, &pr.base_repo, stage, &pr.head_sha)
                == PrDetailAvailability::Fresh
        {
            return;
        }
        if let Some(index) = self.pending.iter().position(|request| request.key == key) {
            let mut existing = self
                .pending
                .remove(index)
                .expect("position came from queue");
            existing.repo_path = repo_path;
            existing.base_repo.clone_from(&pr.base_repo);
            existing.head_sha.clone_from(&pr.head_sha);
            existing.priority |= priority;
            if existing.priority {
                self.pending.push_front(existing);
            } else {
                self.pending.push_back(existing);
            }
            return;
        }
        let slot = self.slots.entry(key.clone()).or_default();
        if slot.head_sha != pr.head_sha {
            slot.generation = slot.generation.wrapping_add(1);
            slot.head_sha.clone_from(&pr.head_sha);
            slot.availability = PrDetailAvailability::Missing;
            slot.refreshed_at = None;
            slot.next_retry_at = None;
            slot.error = None;
        }
        if slot
            .next_retry_at
            .is_some_and(|retry_at| retry_at > Instant::now())
        {
            return;
        }
        slot.availability = PrDetailAvailability::Loading;
        let request = QueuedDetail {
            key,
            repo_path,
            base_repo: pr.base_repo.clone(),
            head_sha: pr.head_sha.clone(),
            priority,
        };
        if priority {
            self.pending.push_front(request);
        } else {
            self.pending.push_back(request);
        }
    }

    fn take_next(&mut self, now: Instant) -> Option<StartedDetail> {
        if self.active.len() >= DETAIL_CONCURRENCY {
            return None;
        }
        let index = self.pending.iter().position(|request| {
            !self.active.iter().any(|(key, _)| key == &request.key)
                && self
                    .slots
                    .get(&request.key)
                    .and_then(|slot| slot.next_retry_at)
                    .is_none_or(|retry_at| retry_at <= now)
        })?;
        let request = self.pending.remove(index)?;
        let slot = self.slots.entry(request.key.clone()).or_default();
        slot.generation = slot.generation.wrapping_add(1);
        slot.head_sha.clone_from(&request.head_sha);
        slot.availability = PrDetailAvailability::Loading;
        slot.error = None;
        let generation = slot.generation;
        self.active.insert((request.key.clone(), generation));
        Some(StartedDetail {
            request,
            generation,
        })
    }

    fn settle(
        &mut self,
        started: &StartedDetail,
        result: &Result<DetailResult, kagi_git::github::PrFetchError>,
        now: Instant,
    ) -> bool {
        self.active
            .remove(&(started.request.key.clone(), started.generation));
        let Some(slot) = self.slots.get_mut(&started.request.key) else {
            return false;
        };
        if slot.generation != started.generation || slot.head_sha != started.request.head_sha {
            return false;
        }
        match result {
            Ok(_) => {
                slot.availability = PrDetailAvailability::Fresh;
                slot.refreshed_at = Some(now);
                slot.successful_head = Some(started.request.head_sha.clone());
                slot.next_retry_at = None;
                slot.error = None;
            }
            Err(error) => {
                slot.availability =
                    if slot.successful_head.as_deref() == Some(started.request.head_sha.as_str()) {
                        PrDetailAvailability::Stale
                    } else {
                        PrDetailAvailability::Missing
                    };
                slot.next_retry_at = Some(now + RETRY_DELAY);
                slot.error = Some(error.to_string());
            }
        }
        true
    }

    fn reconcile_heads(&mut self, prs: &[PullRequest]) {
        let listed: HashSet<u64> = prs.iter().map(|pr| pr.number).collect();
        self.pending.retain(|request| {
            listed.contains(&request.key.number) || self.opened.contains(&request.key.number)
        });
        for pr in prs {
            for stage in [PrDetailStage::Status, PrDetailStage::Body] {
                let key = DetailKey {
                    base_repo: pr.base_repo.clone(),
                    number: pr.number,
                    stage,
                };
                let Some(slot) = self.slots.get_mut(&key) else {
                    continue;
                };
                if slot.head_sha != pr.head_sha {
                    slot.generation = slot.generation.wrapping_add(1);
                    slot.head_sha.clone_from(&pr.head_sha);
                    slot.availability = PrDetailAvailability::Missing;
                    slot.refreshed_at = None;
                    slot.next_retry_at = None;
                    slot.error = None;
                }
            }
        }
    }

    fn targets(&self) -> (BTreeSet<u64>, BTreeSet<u64>) {
        (self.visible.clone(), self.opened.clone())
    }
}

#[derive(Debug)]
enum DetailResult {
    Status(PrStatusDetail),
    Body(PrBodyDetail),
}

fn apply_status_copies<'a>(
    list: &mut [PullRequest],
    opened: impl IntoIterator<Item = &'a mut PullRequest>,
    detail: &PrStatusDetail,
) {
    for pr in list.iter_mut() {
        kagi_domain::github::apply_pr_status(pr, detail);
    }
    for pr in opened {
        kagi_domain::github::apply_pr_status(pr, detail);
    }
}

fn apply_body_copies<'a>(
    list: &mut [PullRequest],
    opened: impl IntoIterator<Item = &'a mut PullRequest>,
    detail: &PrBodyDetail,
) {
    for pr in list.iter_mut() {
        kagi_domain::github::apply_pr_body(pr, detail);
    }
    for pr in opened {
        kagi_domain::github::apply_pr_body(pr, detail);
    }
}

impl KagiApp {
    pub(super) fn pr_status_availability(&self, pr: &PullRequest) -> PrDetailAvailability {
        self.ui().pr_details.availability(
            pr.number,
            &pr.base_repo,
            PrDetailStage::Status,
            &pr.head_sha,
        )
    }

    /// Called by the dashboard `uniform_list` processor after layout identifies
    /// the visible range. Repeated frames with the same PR number set are free.
    pub(super) fn observe_visible_prs(&mut self, numbers: BTreeSet<u64>, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo_path)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let opened: BTreeSet<u64> = self
            .ui()
            .pr_mode
            .as_ref()
            .map(|mode| mode.tabs.iter().map(|tab| tab.pr.number).collect())
            .unwrap_or_default();
        let (timer, epoch) = match self.ui.get_mut(&owner) {
            Some(ui) => {
                let epoch = ui.github_prs_epoch;
                (
                    ui.pr_details
                        .observe_visible(numbers, opened, epoch, Instant::now()),
                    epoch,
                )
            }
            None => return,
        };
        let Some((generation, delay)) = timer else {
            return;
        };
        cx.spawn(async move |this, acx| {
            acx.background_executor().timer(delay).await;
            let _ = this.update(acx, |app, cx| {
                let visible = app.ui.get_mut(&owner).and_then(|ui| {
                    let is_home = ui
                        .pr_mode
                        .as_ref()
                        .is_some_and(|mode| mode.active.is_none());
                    is_home
                        .then(|| ui.pr_details.finish_visible_debounce(generation, epoch))
                        .flatten()
                });
                let Some(visible) = visible else { return };
                app.enqueue_pr_details(owner, repo_path, &visible, false, false, cx);
            });
        })
        .detach();
    }

    /// An opened PR bypasses the visible-row debounce and requests both levels.
    pub(super) fn prioritize_pr_details(&mut self, number: u64, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo_path)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        if let Some(ui) = self.ui.get_mut(&owner) {
            let opened = ui
                .pr_mode
                .as_ref()
                .map(|mode| mode.tabs.iter().map(|tab| tab.pr.number).collect())
                .unwrap_or_default();
            ui.pr_details.leave_home(opened);
        }
        let numbers = BTreeSet::from([number]);
        self.enqueue_pr_details(owner, repo_path, &numbers, true, false, cx);
    }

    /// Re-arm the currently relevant reads after an L1 answer. L2 is volatile,
    /// and an opened PR also refreshes its body/statistics.
    pub(super) fn refresh_pr_detail_targets(
        &mut self,
        owner: crate::app::SessionId,
        repo_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let (visible, opened) = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            let opened: BTreeSet<u64> = ui
                .pr_mode
                .as_ref()
                .map(|mode| mode.tabs.iter().map(|tab| tab.pr.number).collect())
                .unwrap_or_default();
            ui.pr_details.opened = opened;
            ui.pr_details.reconcile_heads(&ui.github_prs);
            ui.pr_details.targets()
        };
        self.enqueue_pr_details(owner, repo_path.clone(), &visible, false, true, cx);
        self.enqueue_pr_details(owner, repo_path, &opened, true, true, cx);
    }

    fn enqueue_pr_details(
        &mut self,
        owner: crate::app::SessionId,
        repo_path: PathBuf,
        numbers: &BTreeSet<u64>,
        include_body: bool,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        let prs: Vec<PullRequest> = numbers
            .iter()
            .filter_map(|number| {
                ui.github_prs
                    .iter()
                    .find(|pr| pr.number == *number)
                    .cloned()
                    .or_else(|| {
                        ui.pr_mode.as_ref().and_then(|mode| {
                            mode.tabs
                                .iter()
                                .find(|tab| tab.pr.number == *number)
                                .map(|tab| tab.pr.clone())
                        })
                    })
            })
            .collect();
        for pr in &prs {
            ui.pr_details.enqueue(
                repo_path.clone(),
                pr,
                PrDetailStage::Status,
                include_body,
                force,
            );
            if include_body {
                ui.pr_details
                    .enqueue(repo_path.clone(), pr, PrDetailStage::Body, true, force);
            }
        }
        self.pump_pr_detail_queue(owner, cx);
    }

    fn pump_pr_detail_queue(&mut self, owner: crate::app::SessionId, cx: &mut Context<Self>) {
        loop {
            let started = self
                .ui
                .get_mut(&owner)
                .and_then(|ui| ui.pr_details.take_next(Instant::now()));
            let Some(started) = started else { break };
            cx.spawn(async move |this, acx| {
                let request = started.request.clone();
                let workdir = request.repo_path.clone();
                let base_repo = request.base_repo.clone();
                let number = request.key.number;
                let result = acx
                    .background_executor()
                    .spawn(async move {
                        match request.key.stage {
                            PrDetailStage::Status => {
                                kagi_git::github::pr_status_detail(&workdir, &base_repo, number)
                                    .map(DetailResult::Status)
                            }
                            PrDetailStage::Body => {
                                kagi_git::github::pr_body_detail(&workdir, &base_repo, number)
                                    .map(DetailResult::Body)
                            }
                        }
                    })
                    .await;
                let _ = this.update(acx, |app, cx| {
                    app.finish_pr_detail(owner, &started, result, cx);
                    app.pump_pr_detail_queue(owner, cx);
                });
            })
            .detach();
        }
    }

    fn finish_pr_detail(
        &mut self,
        owner: crate::app::SessionId,
        started: &StartedDetail,
        result: Result<DetailResult, kagi_git::github::PrFetchError>,
        cx: &mut Context<Self>,
    ) {
        let owner_is_active = self.active_session() == Some(owner);
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        if !ui.pr_details.settle(started, &result, Instant::now()) {
            return;
        }
        if let Ok(detail) = result {
            match detail {
                DetailResult::Status(detail) => {
                    let opened = ui
                        .pr_mode
                        .iter_mut()
                        .flat_map(|mode| mode.tabs.iter_mut().map(|tab| &mut tab.pr));
                    apply_status_copies(&mut ui.github_prs, opened, &detail);
                }
                DetailResult::Body(detail) => {
                    let opened = ui
                        .pr_mode
                        .iter_mut()
                        .flat_map(|mode| mode.tabs.iter_mut().map(|tab| &mut tab.pr));
                    apply_body_copies(&mut ui.github_prs, opened, &detail);
                }
            }
            ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
        }
        if owner_is_active {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(controller.settle(&first, &ok, Instant::now()));
        controller.enqueue(path(), &pr, PrDetailStage::Status, false, true);
        let refresh = controller.take_next(Instant::now()).unwrap();
        let failed = Err(kagi_git::github::PrFetchError::Network("offline".into()));
        assert!(controller.settle(&refresh, &failed, Instant::now()));
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
        assert!(!controller.settle(&old, &result, Instant::now()));
        assert_eq!(
            controller.availability(1, "github.com/acme/widgets", PrDetailStage::Status, "new"),
            PrDetailAvailability::Loading
        );
        assert!(controller.take_next(Instant::now()).is_some());
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
}
