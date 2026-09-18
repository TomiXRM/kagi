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
    use kagi_domain::github::{CiState, Mergeable, PullRequest, ReviewState};
    PullRequest {
        number,
        title: title.to_string(),
        head: head.to_string(),
        head_sha: format!("{number:040x}"),
        base: "main".to_string(),
        is_draft: false,
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: format!("https://github.com/example/repo/pull/{number}"),
        author: "alice".to_string(),
        reviewers: Vec::new(),
        body: String::new(),
        checks: Vec::new(),
        mergeable: Mergeable::Clean,
        cross_repository: false,
        base_repo: "github.com/example/repo".to_string(),
    }
}
