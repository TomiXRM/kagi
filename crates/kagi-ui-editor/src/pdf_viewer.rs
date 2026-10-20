//! Continuous, viewport-driven presentation over the shared PDF raster owner.
use super::*;
use crate::pdf::{
    raster_scale, render_page_with_budget, status_message, PdfError, MAX_ZOOM, MIN_ZOOM,
};
use gpui_component::Disableable as _;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// Caps include old-density images retained during zoom. No per-document bitmap.
const MAX_CACHED_BYTES: usize = 64_000_000;
const MAX_CACHED_PIXELS: usize = MAX_CACHED_BYTES / 4;
const MAX_RASTER_JOBS: usize = 2;
const MAX_PAGE_PIXELS: usize = MAX_CACHED_PIXELS * 9 / 10;
const PREVIEW_PIXELS: usize = 500_000;
const RETAINED_PAGE_ENTRIES: usize = 32;
const REFINE_AFTER: Duration = Duration::from_millis(120);
static RASTER_JOBS: AtomicUsize = AtomicUsize::new(0);

struct RasterPermit;
impl RasterPermit {
    fn acquire() -> Option<Self> {
        RASTER_JOBS
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |count| {
                (count < MAX_RASTER_JOBS).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for RasterPermit {
    fn drop(&mut self) {
        RASTER_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct PageLayout {
    top: f32,
    width: f32,
    height: f32,
    error: Option<PdfError>,
}
pub(super) struct PdfLayout {
    pages: Vec<PageLayout>,
    width: f32,
    height: f32,
    gap: f32,
    widest_page: f32,
}
impl PdfLayout {
    pub(super) fn new(source: &PdfSource, zoom: f32, gap: f32) -> Self {
        let gap = gap.max(1.);
        let mut result = Self {
            pages: Vec::with_capacity(source.dimensions.len()),
            width: 0.,
            height: 0.,
            gap,
            widest_page: 0.,
        };
        for &(width, height) in source.dimensions.iter() {
            if width.is_finite() && height.is_finite() && width > 0. && height > 0. {
                result.widest_page = result.widest_page.max(width);
            }
            let error = raster_scale(width, height, zoom, 1.).err();
            let (width, height) = if error.is_some() {
                (300., 120.)
            } else {
                (width * zoom, height * zoom)
            };
            result.pages.push(PageLayout {
                top: result.height,
                width,
                height,
                error,
            });
            result.width = result.width.max(width);
            result.height += height + gap;
        }
        result.height = (result.height - gap).max(0.);
        result
    }
    fn fit_width_zoom(&self, viewport_width: f32, padding: f32) -> Option<f32> {
        let available = viewport_width - 2. * padding;
        if !viewport_width.is_finite()
            || !padding.is_finite()
            || padding < 0.
            || available <= 0.
            || self.widest_page <= 0.
        {
            return None;
        }
        let zoom = (available / self.widest_page).min(MAX_ZOOM);
        (zoom.is_finite() && zoom > 0.).then_some(zoom)
    }
    pub(super) fn page_at(&self, y: f32) -> usize {
        self.pages
            .partition_point(|page| page.top <= y)
            .saturating_sub(1)
    }
    fn visible(&self, top: f32, height: f32) -> Range<usize> {
        if self.pages.is_empty() {
            return 0..0;
        }
        let first = self.page_at(top.max(0.));
        let end = self
            .pages
            .partition_point(|page| page.top < top + height.max(1.));
        first..end.max(first + 1).min(self.pages.len())
    }
    fn fill_desired(
        &self,
        source: &PdfSource,
        current: usize,
        visible: Range<usize>,
        (zoom, scale): (f32, f32),
        desired: &mut Vec<RasterDemand>,
    ) {
        desired.clear();
        if visible.is_empty() {
            return;
        }
        let prefetch = [
            (visible.end < self.pages.len()).then_some(visible.end),
            visible.start.checked_sub(1),
            (visible.end + 1 < self.pages.len()).then_some(visible.end + 1),
        ];
        let prefetch_count = prefetch.iter().flatten().count();
        let visible_budget = MAX_CACHED_PIXELS - prefetch_count * PREVIEW_PIXELS;
        let demand = |page: usize, budget: usize| {
            let (width, height) = source.dimensions[page];
            let full_pixels = raster_scale(width, height, zoom, scale).map_or(budget, |scale| {
                ((width * scale).ceil() as usize)
                    .saturating_mul((height * scale).ceil() as usize)
                    .max(1)
            });
            RasterDemand {
                page,
                pixel_budget: budget.min(full_pixels),
            }
        };
        for page in std::iter::once(current)
            .filter(|page| visible.contains(page))
            .chain(visible.clone().filter(|page| *page != current))
        {
            desired.push(demand(page, MAX_CACHED_PIXELS));
        }
        // Measure native demand once into the reused Vec. Stable density tiers
        // only apply under real pixel pressure, not merely a changed page count
        // or a desire to retain more warm pages. Mixed small/large pages can use
        // their full density whenever their complete visible working set fits.
        let mut page_budget = if prefetch_count == 0 {
            MAX_CACHED_PIXELS
        } else {
            MAX_PAGE_PIXELS
        };
        while page_budget > 1
            && desired
                .iter()
                .map(|demand| demand.pixel_budget.min(page_budget))
                .sum::<usize>()
                > visible_budget
        {
            page_budget = (page_budget / 2).max(1);
        }
        for demand in desired.iter_mut() {
            demand.pixel_budget = demand.pixel_budget.min(page_budget);
        }
        // Prefetch the same cheap preview shown by a cold visible page. It is
        // immediately usable on entry, unlike the former tiny, disposable share.
        // Keep both directions useful, with one extra page ahead.
        for page in prefetch.into_iter().flatten() {
            desired.push(demand(page, PREVIEW_PIXELS.min(page_budget)));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RasterDemand {
    page: usize,
    pixel_budget: usize,
}

struct CachedPage {
    page: usize,
    generation: u64,
    pixel_budget: usize,
    image: Option<Arc<gpui::RenderImage>>,
    error: Option<PdfError>,
    partial: bool,
    bytes: usize,
}

impl CachedPage {
    fn satisfies(&self, demand: RasterDemand, generation: u64) -> bool {
        self.page == demand.page
            && self.generation == generation
            && (self.error.is_some() || self.pixel_budget >= demand.pixel_budget)
    }
}

fn next_raster(
    desired: &[RasterDemand],
    visible: &Range<usize>,
    cache: &VecDeque<CachedPage>,
    generation: u64,
    refine: bool,
) -> Option<RasterDemand> {
    // All newly visible pages get a usable image before density refinement or
    // speculative work. Old-density images stay visible until a settled refine.
    for demand in desired
        .iter()
        .filter(|demand| visible.contains(&demand.page))
    {
        let entry = cache.iter().find(|entry| entry.page == demand.page);
        if entry.is_none_or(|entry| {
            entry.image.is_none() && (entry.generation != generation || entry.error.is_none())
        }) {
            return Some(RasterDemand {
                page: demand.page,
                pixel_budget: demand.pixel_budget.min(PREVIEW_PIXELS),
            });
        }
    }
    if refine {
        if let Some(demand) = desired
            .iter()
            .filter(|demand| visible.contains(&demand.page))
            .find(|demand| {
                !cache
                    .iter()
                    .any(|entry| entry.satisfies(**demand, generation))
            })
        {
            return Some(*demand);
        }
    }
    desired
        .iter()
        .filter(|demand| !visible.contains(&demand.page))
        .find(|demand| {
            !cache
                .iter()
                .any(|entry| entry.satisfies(**demand, generation))
        })
        .copied()
}

fn insert_cached_page(
    cache: &mut VecDeque<CachedPage>,
    entry: CachedPage,
    desired: &[RasterDemand],
    generation: u64,
    mut retire: impl FnMut(Arc<gpui::RenderImage>),
) {
    if let Some(index) = cache.iter().position(|old| old.page == entry.page) {
        if let Some(image) = cache.remove(index).and_then(|old| old.image) {
            retire(image);
        }
    }
    let mut bytes = cache.iter().map(|old| old.bytes).sum::<usize>() + entry.bytes;
    let limit = desired.len().max(RETAINED_PAGE_ENTRIES);
    while bytes > MAX_CACHED_BYTES || cache.len() >= limit {
        // LRU pages outside current demand go first. Obsolete or over-budget
        // densities cannot evict the complete current set (which fits by design).
        let index = cache
            .iter()
            .rposition(|old| !desired.iter().any(|demand| demand.page == old.page))
            .or_else(|| cache.iter().rposition(|old| old.generation != generation))
            .or_else(|| {
                cache.iter().rposition(|old| {
                    desired.iter().any(|demand| {
                        demand.page == old.page && old.pixel_budget > demand.pixel_budget
                    })
                })
            })
            .unwrap_or_else(|| cache.len().saturating_sub(1));
        let Some(old) = cache.remove(index) else {
            break;
        };
        bytes -= old.bytes;
        if let Some(image) = old.image {
            retire(image);
        }
    }
    cache.push_front(entry);
}

fn render_if_current<T>(
    revision: &AtomicU64,
    expected: u64,
    render: impl FnOnce() -> T,
) -> Option<T> {
    (revision.load(Ordering::Acquire) == expected).then(render)
}

// Source pointer and explicit raster parameters defend replacement and zoom ABA.
pub(super) struct RenderIdentity {
    generation: u64,
    source: Arc<Vec<u8>>,
    page: usize,
    zoom: f32,
    backing_scale: f32,
    pixel_budget: usize,
}
impl RenderIdentity {
    fn matches(
        &self,
        source: &PdfSource,
        generation: u64,
        zoom: f32,
        backing_scale: Option<f32>,
        desired: &[RasterDemand],
    ) -> bool {
        self.generation == generation
            && Arc::ptr_eq(&self.source, &source.bytes)
            && self.zoom == zoom
            && backing_scale == Some(self.backing_scale)
            && desired
                .iter()
                .any(|demand| demand.page == self.page && self.pixel_budget <= demand.pixel_budget)
    }
}

/// A viewport-top anchor in logical page coordinates, independent of zoom/DPI.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PdfScrollPosition {
    pub page: usize,
    pub page_fraction: f32,
    pub horizontal_fraction: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PdfScrollChanged {
    pub position: PdfScrollPosition,
}

/// A retained, read-only PDF viewport shared by Editor and historical previews.
/// The parent must provide a bounded flex viewport; no disk or Git I/O occurs here.
pub struct PdfViewer {
    pub(super) source: PdfSource,
    focus: gpui::FocusHandle,
    focus_scope: gpui::FocusHandle,
    scroll: gpui::ScrollHandle,
    layout: PdfLayout,
    zoom: f32,
    backing_scale: Option<f32>,
    generation: u64,
    page: usize,
    navigation: Option<(usize, gpui::Point<gpui::Pixels>)>,
    visible: Range<usize>,
    mounted: Range<usize>,
    desired: Vec<RasterDemand>,
    cache: VecDeque<CachedPage>,
    pending: bool,
    demand_revision: Arc<AtomicU64>,
    motion_offset: gpui::Point<gpui::Pixels>,
    reported_offset: gpui::Point<gpui::Pixels>,
    synchronized_position: Option<PdfScrollPosition>,
    last_motion: Instant,
    refinement_wake_pending: bool,
    #[cfg(feature = "gui-e2e")]
    raster_requests_started: usize,
}
impl PdfViewer {
    pub fn new(source: PdfSource, cx: &mut Context<Self>) -> Self {
        let layout = PdfLayout::new(&source, 1., f32::from(theme::scaled_px(16.)));
        cx.on_release(|viewer, cx| {
            viewer.demand_revision.fetch_add(1, Ordering::Release);
            for entry in viewer.cache.drain(..) {
                if let Some(image) = entry.image {
                    // Defer until any currently updated Window is back in App.
                    cx.defer(move |cx| cx.drop_image(image, None));
                }
            }
        })
        .detach();
        Self {
            source,
            focus: cx.focus_handle(),
            focus_scope: cx.focus_handle(),
            scroll: gpui::ScrollHandle::new(),
            layout,
            zoom: 1.,
            backing_scale: None,
            generation: 1,
            page: 0,
            navigation: None,
            visible: 0..0,
            mounted: 0..0,
            desired: Vec::new(),
            cache: VecDeque::new(),
            pending: false,
            demand_revision: Arc::new(AtomicU64::new(1)),
            motion_offset: gpui::point(px(0.), px(0.)),
            reported_offset: gpui::point(px(0.), px(0.)),
            synchronized_position: None,
            last_motion: cx.background_executor().now(),
            refinement_wake_pending: false,
            #[cfg(feature = "gui-e2e")]
            raster_requests_started: 0,
        }
    }

    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    /// Focus subtree for a host that needs to remember the active comparison side.
    /// The body retains its independent tabbable Focusable handle.
    pub fn focus_scope_handle(&self) -> gpui::FocusHandle {
        self.focus_scope.clone()
    }

    pub fn scroll_position(&self) -> PdfScrollPosition {
        let top = -f32::from(self.scroll.offset().y);
        let page = self.layout.page_at(top);
        let page_fraction = self
            .layout
            .pages
            .get(page)
            .map_or(0., |page| ((top - page.top) / page.height).clamp(0., 1.));
        let max_x = f32::from(self.scroll.max_offset().x);
        PdfScrollPosition {
            page,
            page_fraction,
            horizontal_fraction: if max_x > 0. {
                (-f32::from(self.scroll.offset().x) / max_x).clamp(0., 1.)
            } else {
                0.
            },
        }
    }

    /// Apply a peer anchor without changing zoom, focus, or emitting an echo.
    pub fn apply_scroll_position(&mut self, position: PdfScrollPosition, cx: &mut Context<Self>) {
        if self.layout.pages.is_empty() {
            return;
        }
        self.synchronized_position = Some(position);
        self.apply_synchronized_position(position);
        self.sync_viewport(cx);
        cx.notify();
    }

    fn apply_synchronized_position(&mut self, position: PdfScrollPosition) {
        let fraction = |value: f32| {
            if value.is_finite() {
                value.clamp(0., 1.)
            } else {
                0.
            }
        };
        let index = position.page.min(self.layout.pages.len().saturating_sub(1));
        let Some(page) = self.layout.pages.get(index) else {
            return;
        };
        let max = self.scroll.max_offset();
        let top = page.top + page.height * fraction(position.page_fraction);
        self.navigation = None;
        self.scroll.set_offset(gpui::point(
            -max.x * fraction(position.horizontal_fraction),
            px(-top).clamp(-max.y, px(0.)),
        ));
        self.reported_offset = self.scroll.offset();
    }

    fn publish_scroll(&mut self, cx: &mut Context<Self>) {
        let offset = self.scroll.offset();
        if offset != self.reported_offset {
            self.reported_offset = offset;
            cx.emit(PdfScrollChanged {
                position: self.scroll_position(),
            });
        }
    }

    pub(super) fn replace_source(&mut self, source: PdfSource, cx: &mut Context<Self>) {
        if Arc::ptr_eq(&self.source.bytes, &source.bytes) {
            return;
        }
        let page = self.page;
        self.source = source;
        self.layout = PdfLayout::new(&self.source, self.zoom, self.layout.gap);
        self.generation = self.generation.wrapping_add(1);
        self.demand_revision.fetch_add(1, Ordering::Release);
        for entry in self.cache.drain(..) {
            if let Some(image) = entry.image {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
        }
        self.page = page.min(self.layout.pages.len().saturating_sub(1));
        self.navigation = None;
        let top = self.layout.pages.get(self.page).map_or(0., |page| page.top);
        self.scroll.set_offset(gpui::point(px(0.), px(-top)));
        self.reported_offset = self.scroll.offset();
        self.synchronized_position = None;
        self.sync_viewport(cx);
        cx.notify();
    }

    // The single scroll child is the logical document; only viewport pages mount.
    fn document_center(&self) -> gpui::Point<gpui::Pixels> {
        let origin = self
            .scroll
            .bounds_for_item(0)
            .map_or(self.scroll.bounds().origin, |bounds| bounds.origin);
        self.scroll.bounds().center() - origin - self.scroll.offset()
    }

    fn fit_width_zoom(&self) -> Option<f32> {
        let viewport = self.scroll.bounds();
        let document = self.scroll.bounds_for_item(0)?;
        // The unscrolled inset includes both scaled padding and the focus ring.
        self.layout.fit_width_zoom(
            f32::from(viewport.size.width),
            f32::from(document.left() - viewport.left()),
        )
    }

    pub(super) fn change_pdf(
        &mut self,
        delta: isize,
        factor: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        if self.source.document.is_err() || !factor.is_finite() || factor <= 0. {
            return;
        }
        let page = self
            .page
            .saturating_add_signed(delta)
            .min(self.layout.pages.len() - 1);
        let zoom = if reset {
            1.
        } else {
            // Below the manual floor, Fit's scale remains a valid navigation
            // scale and Zoom In grows from it instead of jumping to 25%.
            (self.zoom * factor).clamp(self.zoom.min(MIN_ZOOM), MAX_ZOOM)
        };
        if zoom != self.zoom {
            self.apply_zoom(zoom, cx);
            return;
        } else if page != self.page {
            let offset = gpui::point(
                self.scroll.offset().x,
                px(-self.layout.pages[page].top).clamp(-self.scroll.max_offset().y, px(0.)),
            );
            self.scroll.set_offset(offset);
            // Short/mixed pages can share a clamped scroll position. Preserve
            // the revealed toolbar target until the reader actually scrolls.
            self.navigation = Some((page, offset));
            self.page = page;
        } else {
            return;
        }
        self.sync_viewport(cx);
        cx.notify();
    }

    fn apply_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        if !zoom.is_finite()
            || zoom <= 0.
            || zoom > MAX_ZOOM
            || zoom == self.zoom
            || self.layout.pages.is_empty()
            || self.source.document.is_err()
        {
            return;
        }
        self.navigation = None;
        let center = self.document_center();
        let anchor = self.layout.page_at(f32::from(center.y));
        let within = f32::from(center.y) - self.layout.pages[anchor].top;
        // A mixed-width document's center can lie outside its narrower page.
        let anchor_x = f32::from(center.x);
        let anchor_x = if anchor_x > self.layout.pages[anchor].width {
            self.layout.pages[anchor].width / 2.
        } else {
            anchor_x
        };
        let ratio = f64::from(zoom) / f64::from(self.zoom);
        self.zoom = zoom;
        self.layout = PdfLayout::new(&self.source, zoom, self.layout.gap);
        // A tiny fitted scale must also safely survive a subsequent 100% reset.
        let new_x = (f64::from(anchor_x) * ratio).clamp(0., f64::from(self.layout.width)) as f32;
        let new_y = (f64::from(self.layout.pages[anchor].top) + f64::from(within) * ratio)
            .clamp(0., f64::from(self.layout.height)) as f32;
        let displacement = gpui::point(px(new_x), px(new_y)) - center;
        self.scroll.set_offset(self.scroll.offset() - displacement);
        self.generation = self.generation.wrapping_add(1);
        self.demand_revision.fetch_add(1, Ordering::Release);
        self.sync_viewport(cx);
        cx.notify();
    }

    fn sync_viewport(&mut self, cx: &mut Context<Self>) {
        let previous_visible = self.visible.clone();
        let offset = self.scroll.offset();
        if offset != self.motion_offset {
            self.motion_offset = offset;
            self.last_motion = cx.background_executor().now();
        }
        let top = -f32::from(self.scroll.offset().y);
        let height = f32::from(self.scroll.bounds().size.height).max(1.);
        self.visible = self.layout.visible(top, height);
        self.mounted = self.visible.start.saturating_sub(1)
            ..(self.visible.end + 1).min(self.layout.pages.len());
        if let Some((page, offset)) = self
            .navigation
            .filter(|(_, offset)| *offset == self.scroll.offset())
        {
            self.page = page;
            self.navigation = Some((page, offset));
        } else {
            self.navigation = None;
            self.page = self.visible.start;
            let max = self.scroll.max_offset().y;
            if max > px(0.) && self.scroll.offset().y <= -max + px(1.) {
                self.page = self.layout.pages.len().saturating_sub(1);
            }
        }
        self.layout.fill_desired(
            &self.source,
            self.page,
            self.visible.clone(),
            (self.zoom, self.backing_scale.unwrap_or(1.)),
            &mut self.desired,
        );
        if self.visible != previous_visible {
            self.demand_revision.fetch_add(1, Ordering::Release);
            // Touch only when the viewport window changes, not on every paint.
            for demand in self.desired.iter().rev() {
                if let Some(index) = self
                    .cache
                    .iter()
                    .position(|entry| entry.page == demand.page)
                {
                    if let Some(entry) = self.cache.remove(index) {
                        self.cache.push_front(entry);
                    }
                }
            }
            while self.cache.len() > self.desired.len().max(RETAINED_PAGE_ENTRIES) {
                if let Some(image) = self.cache.pop_back().and_then(|entry| entry.image) {
                    cx.defer(move |cx| cx.drop_image(image, None));
                }
            }
        }
    }

    fn request_render(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(scale) = self.backing_scale else {
            return;
        };
        let elapsed = cx
            .background_executor()
            .now()
            .saturating_duration_since(self.last_motion);
        let refine = elapsed >= REFINE_AFTER;
        let demand = next_raster(
            &self.desired,
            &self.visible,
            &self.cache,
            self.generation,
            refine,
        );
        let Some(RasterDemand { page, pixel_budget }) = demand else {
            if !refine && !self.refinement_wake_pending {
                self.refinement_wake_pending = true;
                let delay = REFINE_AFTER.saturating_sub(elapsed);
                cx.spawn(async move |viewer, cx| {
                    cx.background_executor().timer(delay).await;
                    let _ = viewer.update(cx, |viewer, cx| {
                        viewer.refinement_wake_pending = false;
                        cx.notify();
                    });
                })
                .detach();
            }
            return;
        };
        let Ok(document) = &self.source.document else {
            return;
        };
        self.pending = true;
        let Some(permit) = RasterPermit::acquire() else {
            // One coalesced retry, not a growing queue of departed page jobs.
            cx.spawn(async move |viewer, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let _ = viewer.update(cx, |viewer, cx| {
                    viewer.pending = false;
                    cx.notify();
                });
            })
            .detach();
            return;
        };
        #[cfg(feature = "gui-e2e")]
        {
            self.raster_requests_started += 1;
        }
        let identity = RenderIdentity {
            generation: self.generation,
            source: self.source.bytes.clone(),
            page,
            zoom: self.zoom,
            backing_scale: scale,
            pixel_budget,
        };
        let document = document.clone();
        let zoom = self.zoom;
        let revision = self.demand_revision.clone();
        let expected_revision = revision.load(Ordering::Acquire);
        let task = cx.background_spawn(async move {
            // A queued task may become obsolete before it receives CPU time.
            render_if_current(&revision, expected_revision, || {
                render_page_with_budget(document, page, zoom, scale, pixel_budget)
            })
        });
        cx.spawn(async move |viewer, cx| {
            let result = task.await;
            // A synchronous Hayro call cannot be cancelled midway. Keep its
            // permit until CPU work and publication finish, even after close.
            let _ = viewer.update(cx, |viewer, cx| {
                viewer.pending = false;
                if let Some(result) = result {
                    viewer.publish_raster(identity, result, cx);
                }
                cx.notify();
            });
            drop(permit);
        })
        .detach();
    }

    fn publish_raster(
        &mut self,
        identity: RenderIdentity,
        result: Result<(Arc<gpui::RenderImage>, bool), PdfError>,
        cx: &mut Context<Self>,
    ) {
        if !identity.matches(
            &self.source,
            self.generation,
            self.zoom,
            self.backing_scale,
            &self.desired,
        ) || self.cache.iter().any(|entry| {
            entry.satisfies(
                RasterDemand {
                    page: identity.page,
                    pixel_budget: identity.pixel_budget,
                },
                self.generation,
            )
        }) {
            return;
        }
        let mut entry = CachedPage {
            page: identity.page,
            generation: identity.generation,
            pixel_budget: identity.pixel_budget,
            image: None,
            error: None,
            partial: false,
            bytes: 0,
        };
        match result {
            Ok((image, partial)) => {
                let size = image.size(0);
                entry.bytes = size.width.0 as usize * size.height.0 as usize * 4;
                if entry.bytes <= identity.pixel_budget * 4 {
                    entry.image = Some(image);
                    entry.partial = partial;
                } else {
                    entry.bytes = 0;
                    entry.error = Some(PdfError::Dimensions);
                }
            }
            Err(error) => entry.error = Some(error),
        }
        insert_cached_page(
            &mut self.cache,
            entry,
            &self.desired,
            self.generation,
            |image| cx.defer(move |cx| cx.drop_image(image, None)),
        );
    }

    fn toolbar(&self, fit_zoom: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let count = self.source.dimensions.len();
        let mut toolbar = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_2()
            .py_1()
            .flex_shrink_0();
        for (id, msg, icon, disabled, delta, factor, reset) in [
            (
                "ews-pdf-previous",
                Msg::EditorPdfPrevious,
                Some(gpui_component::IconName::ChevronLeft),
                count == 0 || self.page == 0,
                -1,
                1.,
                false,
            ),
            (
                "ews-pdf-next",
                Msg::EditorPdfNext,
                Some(gpui_component::IconName::ChevronRight),
                count == 0 || self.page + 1 >= count,
                1,
                1.,
                false,
            ),
            (
                "ews-pdf-zoom-out",
                Msg::EditorPdfZoomOut,
                Some(gpui_component::IconName::Minus),
                count == 0 || self.zoom <= MIN_ZOOM,
                0,
                0.8,
                false,
            ),
            (
                "ews-pdf-zoom-in",
                Msg::EditorPdfZoomIn,
                Some(gpui_component::IconName::Plus),
                count == 0 || self.zoom >= MAX_ZOOM,
                0,
                1.25,
                false,
            ),
            (
                "ews-pdf-reset",
                Msg::EditorPdfResetZoom,
                None,
                count == 0 || self.zoom == 1.,
                0,
                1.,
                true,
            ),
        ] {
            let name: SharedString = msg.t().into();
            let button =
                Button::new(id)
                    .ghost()
                    .small()
                    .compact()
                    .when_some(icon, |button, icon| button.icon(icon))
                    .when(reset, |button| button.child("100%"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |viewer, _, _, cx| {
                        viewer.change_pdf(delta, factor, reset, cx)
                    }));
            toolbar = toolbar.child(kagi_ui_core::tooltip::HoverTooltip::hover_tooltip(
                named_button::NamedButton(button)
                    .role(gpui::Role::Button)
                    .aria_label(name.clone()),
                (id, 0usize),
                name,
            ));
        }
        let fit_name: SharedString = Msg::EditorPdfFitWidth.t().into();
        toolbar = toolbar.child(kagi_ui_core::tooltip::HoverTooltip::hover_tooltip(
            named_button::NamedButton(
                Button::new("ews-pdf-fit-width")
                    .ghost()
                    .small()
                    .compact()
                    .child(fit_name.clone())
                    .disabled(self.source.document.is_err() || fit_zoom.is_none())
                    .on_click(cx.listener(|viewer, _, _, cx| {
                        if let Some(zoom) = viewer.fit_width_zoom() {
                            viewer.apply_zoom(zoom, cx);
                        }
                    })),
            )
            .role(gpui::Role::Button)
            .aria_label(fit_name.clone()),
            ("ews-pdf-fit-width", 0usize),
            fit_name,
        ));
        toolbar
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .text_xs()
                    .whitespace_normal()
                    .child(format!(
                        "{} {} / {} · {}{}% · {}",
                        Msg::EditorPdfPage.t(),
                        if count == 0 { 0 } else { self.page + 1 },
                        count,
                        if self.zoom < 0.01 { "<" } else { "" },
                        ((self.zoom * 100.).round() as u32).max(1),
                        Msg::EditorPdfReadOnly.t()
                    )),
            )
            .into_any_element()
    }

    fn page_element(&self, index: usize) -> AnyElement {
        let page = &self.layout.pages[index];
        let cached = self.cache.iter().find(|entry| entry.page == index);
        let mut element = div()
            .id(("pdf-page", index))
            .absolute()
            .top(px(page.top))
            .left_0()
            .w(px(page.width))
            .h(px(page.height))
            .overflow_hidden();
        if let Some(error) = page.error.or_else(|| cached.and_then(|entry| entry.error)) {
            element = element.child(status_message(error.message()));
        } else if let Some(image) = cached.and_then(|entry| entry.image.clone()) {
            element = element.child(gpui::img(image).w_full().h_full());
        } else {
            element = element.child(status_message(Msg::EditorPdfLoading));
        }
        element.into_any_element()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_probe(&self) -> PdfProbe {
        let cached = self.cache.iter().find(|entry| entry.page == self.page);
        let error = self
            .source
            .document
            .as_ref()
            .err()
            .copied()
            .or_else(|| self.layout.pages.get(self.page).and_then(|page| page.error))
            .or_else(|| cached.and_then(|entry| entry.error));
        PdfProbe {
            page: self.page,
            page_count: self.source.dimensions.len(),
            zoom: self.zoom,
            image: cached.and_then(|entry| entry.image.clone()),
            error: error.map(|error| error.message().t()),
            settled: self.source.document.is_err()
                || (!self.pending
                    && self.desired.iter().all(|demand| {
                        self.cache
                            .iter()
                            .any(|entry| entry.satisfies(*demand, self.generation))
                    })),
            in_flight: self.pending,
            request: self.generation,
            scroll: self.scroll.clone(),
            source_id: Arc::as_ptr(&self.source.bytes) as usize,
            cached_pages: self.cache.iter().map(|entry| entry.page).collect(),
            cached_bytes: self.cache.iter().map(|entry| entry.bytes).sum(),
            cache_entry_limit: self.desired.len().max(RETAINED_PAGE_ENTRIES),
            rendered_pages: self
                .cache
                .iter()
                .filter(|entry| entry.image.is_some())
                .map(|entry| entry.page)
                .collect(),
            raster_pixel_budget: self
                .desired
                .iter()
                .find(|demand| demand.page == self.page)
                .map_or(0, |demand| demand.pixel_budget),
            visible_pages: self.visible.clone(),
            mounted_pages: self.mounted.clone(),
            page_sizes: self.source.dimensions.to_vec(),
            raster_jobs: RASTER_JOBS.load(Ordering::Acquire),
            page_tops: self.layout.pages.iter().map(|page| page.top).collect(),
            raster_requests_started: self.raster_requests_started,
        }
    }
}
impl gpui::EventEmitter<PdfScrollChanged> for PdfViewer {}
impl Focusable for PdfViewer {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}
impl Render for PdfViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = window.scale_factor();
        if self.backing_scale != Some(scale) {
            self.backing_scale = Some(scale);
            self.generation = self.generation.wrapping_add(1);
            self.demand_revision.fetch_add(1, Ordering::Release);
        }
        let gap = f32::from(theme::scaled_px(16.)).max(1.);
        if gap != self.layout.gap {
            self.layout = PdfLayout::new(&self.source, self.zoom, gap);
        }
        self.sync_viewport(cx);
        if self.synchronized_position.is_none() {
            self.publish_scroll(cx);
        }
        self.request_render(cx);
        let fit_zoom = self.fit_width_zoom();
        let toolbar = self.toolbar(fit_zoom, cx);
        let mut body = crate::pdf_keyboard::body(
            div()
                .id("ews-pdf-page-scroll")
                .flex_1()
                .min_h(px(0.))
                .min_w(px(0.))
                .overflow_scroll()
                .track_scroll(&self.scroll)
                .p_4(),
            &self.scroll,
            &self.focus,
        );
        if let Err(error) = self.source.document.as_ref() {
            body = body.child(status_message(error.message()));
        } else {
            body = body.child(
                div()
                    .relative()
                    .w(px(self.layout.width))
                    .h(px(self.layout.height))
                    .flex_shrink_0()
                    .children(self.mounted.clone().map(|index| self.page_element(index))),
            );
        }
        let entity = cx.entity().downgrade();
        let measure = gpui::canvas(
            move |_, _, cx| {
                let _ = entity.update(cx, |viewer, cx| {
                    let before = (viewer.visible.clone(), viewer.page);
                    if let Some(position) = viewer.synchronized_position.take() {
                        viewer.apply_synchronized_position(position);
                    }
                    viewer.sync_viewport(cx);
                    viewer.publish_scroll(cx);
                    viewer.request_render(cx);
                    if before != (viewer.visible.clone(), viewer.page)
                        || fit_zoom != viewer.fit_width_zoom()
                    {
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .id("ews-pdf")
            .track_focus(&self.focus_scope)
            .key_context("EditorReadOnly")
            .flex_1()
            .size_full()
            .min_h(px(0.))
            .min_w(px(0.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(toolbar)
            .when(
                self.cache
                    .iter()
                    .any(|entry| self.visible.contains(&entry.page) && entry.partial),
                |element| element.child(div().text_xs().px_2().child(Msg::EditorPdfPartial.t())),
            )
            .child(
                div()
                    .id("ews-pdf-page-area")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(measure)
                    .when(self.source.document.is_ok(), |area| {
                        area.child(Scrollbar::new(&self.scroll))
                    }),
            )
    }
}

#[cfg(test)]
#[path = "pdf_viewer_tests.rs"]
mod tests;
