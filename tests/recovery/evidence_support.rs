//! Deliver transport results without blocking TestDispatcher on a child process.
use gpui::{Task, VisualTestAppContext};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

pub struct Reply<T>(Arc<Mutex<(Option<T>, Option<Waker>)>>);

impl<T> Reply<T> {
    pub fn send(self, value: T) {
        let waker = {
            let mut state = self.0.lock().expect("evidence reply lock");
            assert!(state.0.replace(value).is_none(), "reply delivered twice");
            state.1.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

pub fn deferred<T: Send + 'static>(cx: &mut VisualTestAppContext) -> (Task<T>, Reply<T>) {
    let state = Arc::new(Mutex::new((None, None)));
    let pending = state.clone();
    let task = cx
        .background_executor
        .spawn(std::future::poll_fn(move |cx| {
            let mut state = pending.lock().expect("evidence reply lock");
            if let Some(value) = state.0.take() {
                Poll::Ready(value)
            } else {
                state.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        }));
    (task, Reply(state))
}

/// A plain, clean pull request — the shape a fetch returns, for scenarios that
/// need GitHub evidence to be already cached.
pub fn pull_request(number: u64, title: &str, head: &str) -> kagi_domain::github::PullRequest {
    use kagi_domain::github::{CiState, IssueState, Mergeable, PullRequest, ReviewState};
    PullRequest {
        // An open PR, as a fetch of the open collection returns it: the shared
        // filter strip's default state is Open, and a row with no state is not
        // in it (#753).
        state: IssueState::Open,
        number,
        title: title.to_string(),
        head: head.to_string(),
        head_sha: format!("{number:040x}"),
        base: "main".to_string(),
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: format!("https://github.com/example/repo/pull/{number}"),
        author: "alice".to_string(),
        mergeable: Mergeable::Clean,
        cross_repository: false,
        base_repo: "github.com/example/repo".to_string(),
        ..Default::default()
    }
}

/// A bounded transport page. Nonempty pages take their resolved identity from
/// the actual fixture rows; empty pages must still name their repository.
pub fn pr_page(
    prs: Vec<kagi_domain::github::PullRequest>,
    empty_base_repo: &str,
    next_cursor: Option<&str>,
) -> kagi_domain::github::PrListSnapshot {
    let base_repo = prs
        .first()
        .map_or(empty_base_repo, |pr| pr.base_repo.as_str())
        .to_owned();
    assert!(!base_repo.is_empty(), "a page needs a resolved repository");
    assert!(prs.iter().all(|pr| pr.base_repo == base_repo));
    kagi_domain::github::PrListSnapshot {
        prs,
        base_repo,
        next_cursor: next_cursor.map(str::to_owned),
    }
}

pub type ScanResult = Result<Vec<kagi_git::ops::SquashLink>, kagi_git::GitError>;
pub type GraphSignature = Vec<(String, usize, usize, Vec<(usize, usize, String, usize)>)>;

pub fn queue_scan(cx: &mut VisualTestAppContext) -> Reply<ScanResult> {
    let (task, reply) = deferred(cx);
    kagi::ui::e2e::queue_squash_scan(task);
    reply
}

pub fn link(
    tip: &kagi_git::CommitId,
    squash: &kagi_git::CommitId,
    branch: &str,
) -> kagi_git::ops::SquashLink {
    kagi_git::ops::SquashLink {
        branch: branch.into(),
        tip: tip.0.clone(),
        squash: squash.0.clone(),
    }
}

pub fn graph_signature(app: &kagi::ui::KagiApp, owner: kagi::app::SessionId) -> GraphSignature {
    app.reads
        .get(Some(owner))
        .rows
        .iter()
        .map(|row| {
            (
                row.id.0.clone(),
                row.lane,
                row.lane_count,
                row.edges
                    .iter()
                    .map(|edge| {
                        (
                            edge.from_lane,
                            edge.to_lane,
                            format!("{:?}", edge.kind),
                            edge.color,
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

pub fn ghost_rows(app: &kagi::ui::KagiApp, owner: kagi::app::SessionId) -> Vec<String> {
    app.reads
        .get(Some(owner))
        .rows
        .iter()
        .filter(|row| {
            row.edges
                .iter()
                .any(|edge| edge.color == kagi::ui::graph_squash::GHOST_COLOR)
        })
        .map(|row| row.id.0.clone())
        .collect()
}
