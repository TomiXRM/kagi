//! #495: the one text-first highlight path for every diff surface.
//!
//! A surface installs a text-only [`MainDiffView`] (`build_main_diff_view`)
//! and renders it through `render_diff_list`, which calls
//! [`ensure_highlight`]. The spans are computed off the UI thread and land
//! only where the host still shows **the same rows under the same theme**:
//!
//! - identity is the row allocation, held as a `Weak` so its address cannot
//!   be reused while the request is out. A new selection, a re-read with new
//!   text, or a close installs other rows (or none), so a late result finds
//!   nothing to land on;
//! - the theme slug is captured at request time and compared at landing, so
//!   a result computed under the previous theme is dropped and the next frame
//!   asks again under the current one.
//!
//! The same input is not highlighted twice: a view records the theme its
//! spans were computed with ([`MainDiffView::highlighted`]), an outstanding
//! request is remembered until it lands, and [`MainDiffView::adopt`] keeps
//! the existing rows when a re-read produced the same text. Keeping the rows
//! also keeps the split projection (`diff_split`), which is keyed by the same
//! allocation, instead of recomputing it.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use gpui::{AppContext as _, Context};

use super::{DiffRow, MainDiffView, RowHighlights};
use crate::ui::theme;

/// An entity that shows [`MainDiffView`]s and can hand them back to have
/// their spans applied.
pub(crate) trait DiffHighlightHost: 'static {
    /// Emit the `[kagi] main-diff: highlight ready` contract line when spans
    /// land (the full-width main diff only).
    const LOG_READY: bool = false;
    /// Offer every diff this host currently shows.
    fn for_each_diff(&mut self, visit: &mut dyn FnMut(&mut MainDiffView));
}

type Request = (Weak<Vec<DiffRow>>, &'static str);

thread_local! {
    /// Requests that are out. Claimed while rendering and released when the
    /// result lands — both on the UI thread.
    static IN_FLIGHT: RefCell<Vec<Request>> = const { RefCell::new(Vec::new()) };
}

fn is_rows(weak: &Weak<Vec<DiffRow>>, rows: &Arc<Vec<DiffRow>>) -> bool {
    std::ptr::eq(weak.as_ptr(), Arc::as_ptr(rows))
}

/// Record a request for `rows` under `slug`; `false` when one is already out.
fn claim(rows: &Arc<Vec<DiffRow>>, slug: &'static str) -> bool {
    IN_FLIGHT.with(|requests| {
        let mut requests = requests.borrow_mut();
        requests.retain(|(weak, _)| weak.strong_count() > 0);
        if requests
            .iter()
            .any(|(weak, s)| *s == slug && is_rows(weak, rows))
        {
            return false;
        }
        requests.push((Arc::downgrade(rows), slug));
        true
    })
}

fn release(rows: &Weak<Vec<DiffRow>>, slug: &'static str) {
    IN_FLIGHT.with(|requests| {
        requests
            .borrow_mut()
            .retain(|(weak, s)| !(*s == slug && weak.ptr_eq(rows)));
    });
}

/// Request `view`'s spans under the active theme unless it already has them
/// or a request for exactly these rows and theme is out. Cheap when there is
/// nothing to do, so every render may call it.
pub(crate) fn ensure_highlight<H: DiffHighlightHost>(view: &MainDiffView, cx: &mut Context<H>) {
    let Some(lang) = view.lang else {
        return;
    };
    let active = theme::theme();
    let slug = active.slug;
    if view.highlighted == Some(slug) || !claim(&view.rows, slug) {
        return;
    }
    #[cfg(feature = "gui-e2e")]
    e2e::RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let rows = view.rows.clone();
    let requested = Arc::downgrade(&view.rows);
    let work = cx.background_spawn(async move {
        super::highlight_rows(&rows, lang, &theme::highlight_theme(active))
    });
    cx.spawn(async move |host, acx| {
        let spans = work.await;
        release(&requested, slug);
        let _ = host.update(acx, |host, cx| {
            // Computed under a theme that is no longer active: drop it; the
            // next frame requests the current theme's spans.
            let mut spans = (theme::theme().slug == slug).then_some(spans);
            let mut landed = false;
            host.for_each_diff(&mut |view| {
                let Some(spans) = spans.take_if(|_| is_rows(&requested, &view.rows)) else {
                    return;
                };
                view.apply_highlights(slug, spans);
                landed = true;
                if H::LOG_READY {
                    klog!(
                        "main-diff: highlight ready {} rows={} lang={}",
                        view.title,
                        view.rows.len(),
                        lang
                    );
                }
            });
            if landed {
                cx.notify();
            } else {
                #[cfg(feature = "gui-e2e")]
                e2e::STALE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
    })
    .detach();
}

impl MainDiffView {
    /// Swap computed spans into the rows. The rows get their own allocation
    /// if a render snapshot still shares them.
    fn apply_highlights(&mut self, slug: &'static str, spans: RowHighlights) {
        let rows = Arc::make_mut(&mut self.rows);
        for (row_i, row_highlights) in spans {
            if let Some(DiffRow::Line { highlights, .. }) = rows.get_mut(row_i) {
                *highlights = row_highlights;
            }
        }
        self.highlighted = Some(slug);
    }

    /// Replace this view with `next`, keeping the current rows — spans,
    /// identity and split projection — when `next` has the same text. A
    /// re-read of an unchanged file (a reload, re-opening the same diff) then
    /// costs neither a highlight nor a projection.
    pub(crate) fn adopt(&mut self, next: MainDiffView) {
        let same = self.lang == next.lang
            && self.images.is_none()
            && next.images.is_none()
            && same_text(&self.rows, &next.rows);
        if same {
            let rows = self.rows.clone();
            let highlighted = self.highlighted;
            *self = next;
            self.rows = rows;
            self.highlighted = highlighted;
        } else {
            *self = next;
        }
    }
}

/// Same rows, ignoring spans.
fn same_text(a: &Arc<Vec<DiffRow>>, b: &Arc<Vec<DiffRow>>) -> bool {
    Arc::ptr_eq(a, b)
        || (a.len() == b.len()
            && a.iter().zip(b.iter()).all(|pair| match pair {
                (DiffRow::HunkHeader(x), DiffRow::HunkHeader(y)) => x == y,
                (DiffRow::Binary, DiffRow::Binary) => true,
                (
                    DiffRow::Line {
                        kind,
                        text,
                        old_lineno,
                        new_lineno,
                        ..
                    },
                    DiffRow::Line {
                        kind: k,
                        text: t,
                        old_lineno: o,
                        new_lineno: n,
                        ..
                    },
                ) => kind == k && text == t && old_lineno == o && new_lineno == n,
                _ => false,
            }))
}

/// Install `next` into an optional diff slot through [`MainDiffView::adopt`].
pub(crate) fn install(slot: &mut Option<MainDiffView>, next: Option<MainDiffView>) {
    match (slot.as_mut(), next) {
        (Some(current), Some(next)) => current.adopt(next),
        (_, next) => *slot = next,
    }
}

/// PR mode keeps a diff per open PR tab, in every session.
impl DiffHighlightHost for crate::ui::KagiApp {
    fn for_each_diff(&mut self, visit: &mut dyn FnMut(&mut MainDiffView)) {
        for ui in self.ui.values_mut() {
            let Some(pr_mode) = ui.pr_mode.as_mut() else {
                continue;
            };
            for tab in &mut pr_mode.tabs {
                if let Some(diff) = tab.diff.as_mut() {
                    visit(diff);
                }
            }
        }
    }
}

/// Tier A counters (#495): highlight runs started, and results that found
/// no matching surface (superseded rows or theme) and were dropped.
#[cfg(feature = "gui-e2e")]
pub mod e2e {
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(super) static RUNS: AtomicUsize = AtomicUsize::new(0);
    pub(super) static STALE: AtomicUsize = AtomicUsize::new(0);

    pub fn highlight_runs() -> usize {
        RUNS.load(Ordering::Relaxed)
    }

    pub fn stale_highlights() -> usize {
        STALE.load(Ordering::Relaxed)
    }

    pub fn split_projections() -> usize {
        crate::ui::diff_split::PROJECTIONS.load(Ordering::Relaxed)
    }
}
