//! GUI E2E witness for backend opens attempted inside a render scope.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    static OPENS: Cell<usize> = const { Cell::new(0) };
}

/// A render scope, confined to the thread whose nesting depth it owns.
#[must_use]
pub struct RenderGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

/// Enter a render scope; nested scopes do not double-count backend opens.
pub fn enter() -> RenderGuard {
    DEPTH.set(DEPTH.get() + 1);
    RenderGuard {
        _thread_bound: PhantomData,
    }
}

impl Drop for RenderGuard {
    fn drop(&mut self) {
        DEPTH.set(DEPTH.get() - 1);
    }
}

pub(super) fn record_open() {
    if DEPTH.get() != 0 {
        OPENS.set(OPENS.get() + 1);
    }
}

/// Backend-open attempts made while a render guard was active on this thread.
/// The count survives guard exit so the GUI harness can inspect it afterward.
pub fn opens() -> usize {
    OPENS.get()
}

/// Clear the witness without changing active render scopes.
pub fn reset() {
    OPENS.set(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_render_scoped_opens_and_preserves_outer_scope() {
        reset();
        record_open();
        assert_eq!(opens(), 0);
        let outer = enter();
        record_open();
        {
            let _inner = enter();
            record_open();
            assert_eq!(opens(), 2);
        }
        record_open();
        assert_eq!(opens(), 3);
        reset();
        record_open();
        assert_eq!(opens(), 1);
        drop(outer);
        record_open();
        assert_eq!(opens(), 1);
        reset();
    }

    #[test]
    fn render_scope_and_count_are_thread_local() {
        reset();
        let _guard = enter();
        record_open();
        std::thread::spawn(|| {
            assert_eq!(opens(), 0);
            record_open();
            assert_eq!(opens(), 0);
            let _guard = enter();
            record_open();
            assert_eq!(opens(), 1);
        })
        .join()
        .unwrap();
        assert_eq!(opens(), 1);
        reset();
    }
}
