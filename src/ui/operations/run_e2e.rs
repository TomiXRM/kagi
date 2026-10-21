//! Run a real blocked transport without blocking GPUI's deterministic dispatcher.
use std::sync::{Arc, Mutex};
use std::task::Poll;

thread_local! {
    static RUN_ON_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl crate::ui::KagiApp {
    pub fn run_next_write_on_thread_for_e2e() {
        RUN_ON_THREAD.with(|flag| assert!(!flag.replace(true)));
    }
}

pub(super) fn take_run_on_thread() -> bool {
    RUN_ON_THREAD.with(|flag| flag.replace(false))
}

pub(super) async fn run<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let state = Arc::new(Mutex::new((None, None::<std::task::Waker>)));
    let pending = state.clone();
    std::thread::spawn(move || {
        let completed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok();
        let waker = {
            let mut state = state.lock().expect("transport reply lock");
            state.0 = Some(completed);
            state.1.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    std::future::poll_fn(move |cx| {
        let mut state = pending.lock().expect("transport reply lock");
        if let Some(completed) = state.0.take() {
            Poll::Ready(completed)
        } else {
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    })
    .await
}
