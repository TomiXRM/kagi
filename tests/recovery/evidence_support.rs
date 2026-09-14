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
