//! Continuous, viewport-driven presentation over the shared PDF raster owner.
use super::*;
use crate::pdf::{
    raster_scale, render_page_with_budget, status_message, PdfError, MAX_ZOOM, MIN_ZOOM,
};
use gpui_component::Disableable as _;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

// Caps include old-density images retained during zoom. No per-document bitmap.
const MAX_CACHED_BYTES: usize = 64_000_000;
const MAX_CACHED_PIXELS: usize = MAX_CACHED_BYTES / 4;
const MAX_RASTER_JOBS: usize = 2;
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
}
impl PdfLayout {
    pub(super) fn new(source: &PdfSource, zoom: f32, gap: f32) -> Self {
        let gap = gap.max(1.);
        let mut result = Self {
            pages: Vec::with_capacity(source.dimensions.len()),
            width: 0.,
            height: 0.,
            gap,
        };
        for &(width, height) in source.dimensions.iter() {
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
        let neighbours =
            usize::from(visible.start > 0) + usize::from(visible.end < self.pages.len());
        // Every visible page gets output, never just the first fixed number.
        // Prefetch shares at most 10% of the retained pixel budget.
        let prefetch_pixels = if neighbours > 0 && visible.len() < MAX_CACHED_PIXELS / 2 {
            MAX_CACHED_PIXELS / 10
        } else {
            0
        };
        let visible_budget = (MAX_CACHED_PIXELS - prefetch_pixels) / visible.len();
        let demand = |page: usize, pixel_budget: usize| {
            let (width, height) = source.dimensions[page];
            let full_pixels = raster_scale(width, height, zoom, scale)
                .map_or(pixel_budget, |scale| {
                    (width * scale).ceil() as usize * (height * scale).ceil() as usize
                });
            // Small pages already at native density should not reraster merely
            // because a neighbour entering the viewport changes the allocation.
            RasterDemand {
                page,
                pixel_budget: pixel_budget.min(full_pixels),
            }
        };
        for page in std::iter::once(current)
            .filter(|page| visible.contains(page))
            .chain(visible.clone().filter(|page| *page != current))
        {
            desired.push(demand(page, visible_budget));
        }
        if prefetch_pixels > 0 {
            for page in visible
                .start
                .checked_sub(1)
                .into_iter()
                .chain((visible.end < self.pages.len()).then_some(visible.end))
            {
                desired.push(demand(page, prefetch_pixels / neighbours));
            }
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
            && desired.contains(&RasterDemand {
                page: self.page,
                pixel_budget: self.pixel_budget,
            })
    }
}

/// A retained, read-only PDF viewport shared by Editor and historical previews.
/// The parent must provide a bounded flex viewport; no disk or Git I/O occurs here.
pub struct PdfViewer {
    pub(super) source: PdfSource,
    focus: gpui::FocusHandle,
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
    #[cfg(feature = "gui-e2e")]
    raster_requests_started: usize,
}
impl PdfViewer {
    pub fn new(source: PdfSource, cx: &mut Context<Self>) -> Self {
        let layout = PdfLayout::new(&source, 1., f32::from(theme::scaled_px(16.)));
        Self {
            source,
            focus: cx.focus_handle(),
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
            #[cfg(feature = "gui-e2e")]
            raster_requests_started: 0,
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
        self.cache.clear();
        self.page = page.min(self.layout.pages.len().saturating_sub(1));
        self.navigation = None;
        let top = self.layout.pages.get(self.page).map_or(0., |page| page.top);
        self.scroll.set_offset(gpui::point(px(0.), px(-top)));
        self.sync_viewport();
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

    pub(super) fn change_pdf(
        &mut self,
        delta: isize,
        factor: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        if self.source.document.is_err() {
            return;
        }
        let page = self
            .page
            .saturating_add_signed(delta)
            .min(self.layout.pages.len() - 1);
        let zoom = if reset {
            1.
        } else {
            (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM)
        };
        if zoom != self.zoom {
            self.navigation = None;
            let center = self.document_center();
            let anchor = self.layout.page_at(f32::from(center.y));
            let within = f32::from(center.y) - self.layout.pages[anchor].top;
            let ratio = zoom / self.zoom;
            self.zoom = zoom;
            self.layout = PdfLayout::new(&self.source, zoom, self.layout.gap);
            let new_y = self.layout.pages[anchor].top + within * ratio;
            let displacement = gpui::point(center.x * (ratio - 1.), px(new_y) - center.y);
            self.scroll.set_offset(self.scroll.offset() - displacement);
            self.generation = self.generation.wrapping_add(1);
        } else if page != self.page {
            // Navigation reveals an existing page in the continuous document.
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
        self.sync_viewport();
        cx.notify();
    }

    fn sync_viewport(&mut self) {
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
        self.cache
            .retain(|entry| self.desired.iter().any(|demand| demand.page == entry.page));
    }

    fn request_render(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(scale) = self.backing_scale else {
            return;
        };
        let Some(demand) = self.desired.iter().copied().find(|demand| {
            !self.cache.iter().any(|entry| {
                entry.page == demand.page
                    && entry.generation == self.generation
                    && entry.pixel_budget == demand.pixel_budget
            })
        }) else {
            return;
        };
        let RasterDemand { page, pixel_budget } = demand;
        let Ok(document) = &self.source.document else {
            return;
        };
        self.pending = true;
        let Some(permit) = RasterPermit::acquire() else {
            // One coalesced retry per retained viewer, not one task per page.
            cx.spawn(async move |viewer, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
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
        let task = cx.background_spawn(async move {
            render_page_with_budget(document, page, zoom, scale, pixel_budget)
        });
        cx.spawn(async move |viewer, cx| {
            let result = task.await;
            // Keep the global permit until publication, including when its
            // viewer was dropped. Dropping a task never pretends CPU work stopped.
            drop(permit);
            let _ = viewer.update(cx, |viewer, cx| {
                viewer.pending = false;
                if identity.matches(
                    &viewer.source,
                    viewer.generation,
                    viewer.zoom,
                    viewer.backing_scale,
                    &viewer.desired,
                ) {
                    let mut entry = CachedPage {
                        page,
                        generation: identity.generation,
                        pixel_budget,
                        image: None,
                        error: None,
                        partial: false,
                        bytes: 0,
                    };
                    match result {
                        Ok((image, partial)) => {
                            let size = image.size(0);
                            entry.bytes = size.width.0 as usize * size.height.0 as usize * 4;
                            if entry.bytes <= pixel_budget * 4 {
                                entry.image = Some(image);
                                entry.partial = partial;
                            } else {
                                entry.bytes = 0;
                                entry.error = Some(PdfError::Dimensions);
                            }
                        }
                        Err(error) => entry.error = Some(error),
                    }
                    viewer.cache.retain(|old| old.page != page);
                    while viewer.cache.iter().map(|entry| entry.bytes).sum::<usize>() + entry.bytes
                        > MAX_CACHED_BYTES
                    {
                        // Retire obsolete-resolution entries before any accepted
                        // current demand. The complete current set fits by design.
                        let obsolete = viewer.cache.iter().position(|old| {
                            old.generation != viewer.generation
                                || !viewer.desired.contains(&RasterDemand {
                                    page: old.page,
                                    pixel_budget: old.pixel_budget,
                                })
                        });
                        if let Some(index) = obsolete {
                            viewer.cache.remove(index);
                        } else {
                            viewer.cache.pop_back();
                        }
                    }
                    viewer.cache.push_front(entry);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
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
        toolbar
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .text_xs()
                    .whitespace_normal()
                    .child(format!(
                        "{} {} / {} · {}% · {}",
                        Msg::EditorPdfPage.t(),
                        if count == 0 { 0 } else { self.page + 1 },
                        count,
                        (self.zoom * 100.).round() as u32,
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
                        self.cache.iter().any(|entry| {
                            entry.page == demand.page
                                && entry.generation == self.generation
                                && entry.pixel_budget == demand.pixel_budget
                        })
                    })),
            in_flight: self.pending,
            request: self.generation,
            scroll: self.scroll.clone(),
            source_id: Arc::as_ptr(&self.source.bytes) as usize,
            cached_pages: self.cache.iter().map(|entry| entry.page).collect(),
            cached_bytes: self.cache.iter().map(|entry| entry.bytes).sum(),
            cache_entry_limit: self.mounted.len(),
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
            self.cache.clear();
        }
        let gap = f32::from(theme::scaled_px(16.)).max(1.);
        if gap != self.layout.gap {
            self.layout = PdfLayout::new(&self.source, self.zoom, gap);
        }
        self.sync_viewport();
        self.request_render(cx);
        let toolbar = self.toolbar(cx);
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
                    viewer.sync_viewport();
                    viewer.request_render(cx);
                    if before != (viewer.visible.clone(), viewer.page) {
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
