//! Current working-tree PDF presentation, owned by the ordinary Editor buffer.
//! hayro is not a hostile-document sandbox: output caps do not bound decoded
//! resources or synchronous interpreter time. No historical PDF renderer lives here.
use super::*;
use gpui_component::Disableable as _;
use hayro::hayro_interpret::{font::FontQuery, InterpreterSettings};
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use std::io::Read as _;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_SIDE: f64 = 4096.;
const MAX_PIXELS: f64 = 16_000_000.;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PdfError {
    Invalid,
    Password,
    Encryption,
    Dimensions,
}
impl PdfError {
    fn message(self) -> Msg {
        match self {
            Self::Invalid => Msg::EditorPdfInvalid,
            Self::Password => Msg::EditorPdfPassword,
            Self::Encryption => Msg::EditorPdfEncryption,
            Self::Dimensions => Msg::EditorPdfDimensions,
        }
    }
}
impl From<LoadPdfError> for PdfError {
    fn from(error: LoadPdfError) -> Self {
        match error {
            LoadPdfError::Decryption(DecryptionError::PasswordProtected) => Self::Password,
            LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm) => Self::Encryption,
            _ => Self::Invalid,
        }
    }
}

#[derive(Clone)]
pub(super) struct PdfSource {
    bytes: Arc<Vec<u8>>,
    document: Result<Arc<Pdf>, PdfError>,
}
pub(super) struct PdfPreview {
    pub(super) source: PdfSource,
    page: usize,
    zoom: f32,
    backing_scale: Option<f32>,
    image: Option<Arc<gpui::RenderImage>>,
    error: Option<PdfError>,
    partial: bool,
    request: u64,
    settled: bool,
    scroll: gpui::ScrollHandle,
}
impl PdfPreview {
    fn new(source: PdfSource, page: usize, zoom: f32) -> Self {
        let count = source.document.as_ref().map_or(0, |pdf| pdf.pages().len());
        let error = source.document.as_ref().err().copied();
        Self {
            source,
            page: page.min(count.saturating_sub(1)),
            zoom,
            backing_scale: None,
            image: None,
            error,
            partial: false,
            request: 0,
            settled: error.is_some(),
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

pub(super) fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}
fn recognizes_pdf(path: &Path, bytes: &[u8]) -> bool {
    // The extension selects the parser even for malformed files. The signature
    // additionally recognizes extensionless/misnamed PDFs, before UTF-8 probing.
    is_pdf_path(path) || bytes.starts_with(b"%PDF-")
}

#[derive(Default)]
pub(super) struct LoadedFile {
    pub(super) text: Option<String>,
    pub(super) is_binary: bool,
    pub(super) image: Option<Arc<gpui::Image>>,
    pub(super) missing: bool,
    pub(super) undecodable: bool,
    pub(super) too_large: bool,
    pub(super) pdf: Option<PdfSource>,
}
pub(super) fn read_file(
    path: &Path,
    image_from_bytes: fn(Vec<u8>) -> Option<Arc<gpui::Image>>,
    previous: Option<PdfSource>,
) -> LoadedFile {
    let mut result = LoadedFile::default();
    let read = (|| -> std::io::Result<Option<Vec<u8>>> {
        if !std::fs::metadata(path)?.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        if metadata.len() > MAX_EDITOR_BYTES {
            return Ok(None);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_EDITOR_BYTES + 1).read_to_end(&mut bytes)?;
        Ok((bytes.len() as u64 <= MAX_EDITOR_BYTES).then_some(bytes))
    })();
    let bytes = match read {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            result.too_large = true;
            return result;
        }
        Err(_) => {
            result.missing = true;
            return result;
        }
    };
    if recognizes_pdf(path, &bytes) {
        result.is_binary = true;
        result.pdf = Some(
            if let Some(previous) = previous.filter(|old| *old.bytes == bytes) {
                previous
            } else {
                let bytes = Arc::new(bytes);
                let document = Pdf::new(bytes.clone())
                    .map_err(PdfError::from)
                    .and_then(|pdf| {
                        if pdf.pages().is_empty() {
                            Err(PdfError::Invalid)
                        } else {
                            Ok(Arc::new(pdf))
                        }
                    });
                PdfSource { bytes, document }
            },
        );
    } else if kagi_domain::checklist::content_looks_binary(
        &bytes[..bytes.len().min(BINARY_PROBE_BYTES)],
    ) {
        result.is_binary = true;
        result.image = image_from_bytes(bytes);
    } else {
        match String::from_utf8(bytes) {
            Ok(text) if text.lines().count() <= MAX_EDITOR_LINES => result.text = Some(text),
            Ok(_) => result.too_large = true,
            Err(_) => result.undecodable = true,
        }
    }
    result
}

// Validate before hayro's internal float -> u16 conversion. Logical page size
// remains zoomed even when raster resolution is capped; scrolling is not contain-fit.
fn raster_scale(width: f32, height: f32, zoom: f32, backing_scale: f32) -> Result<f32, PdfError> {
    if !width.is_finite()
        || !height.is_finite()
        || !zoom.is_finite()
        || width <= 0.
        || height <= 0.
        || !(MIN_ZOOM..=MAX_ZOOM).contains(&zoom)
        || !backing_scale.is_finite()
        || backing_scale <= 0.
    {
        return Err(PdfError::Dimensions);
    }
    let (w, h) = (f64::from(width), f64::from(height));
    // Avoid unusable or unbounded layout dimensions as well as raster dimensions.
    if w * f64::from(zoom) > 100_000. || h * f64::from(zoom) > 100_000. {
        return Err(PdfError::Dimensions);
    }
    let scale = (f64::from(zoom) * f64::from(backing_scale))
        .min(MAX_SIDE / w)
        .min(MAX_SIDE / h)
        .min((MAX_PIXELS / (w * h)).sqrt());
    let scale = scale as f32;
    let (rw, rh) = (width * scale, height * scale);
    if !scale.is_finite()
        || rw < 1.
        || rh < 1.
        || f64::from(rw.floor()) > MAX_SIDE
        || f64::from(rh.floor()) > MAX_SIDE
        || f64::from(rw.floor()) * f64::from(rh.floor()) > MAX_PIXELS
    {
        return Err(PdfError::Dimensions);
    }
    Ok(scale)
}

fn render_page(
    document: Arc<Pdf>,
    page_index: usize,
    zoom: f32,
    backing_scale: f32,
) -> Result<(Arc<gpui::RenderImage>, bool), PdfError> {
    let pages = document.pages();
    let page = pages.get(page_index).ok_or(PdfError::Invalid)?;
    let (width, height) = page.render_dimensions();
    let scale = raster_scale(width, height, zoom, backing_scale)?;
    let mut settings = InterpreterSettings::default();
    let standard = settings.font_resolver.clone();
    settings.font_resolver = Arc::new(move |query| {
        if let FontQuery::Fallback(fallback) = query {
            // Japanese CID collections and explicitly Japanese families only;
            // do not replace unrelated script/standard-font resolution.
            let japanese = fallback
                .character_collection
                .as_ref()
                .is_some_and(|collection| {
                    collection.family == hayro::hayro_interpret::hayro_cmap::CidFamily::AdobeJapan1
                })
                || [
                    &fallback.post_script_name,
                    &fallback.font_name,
                    &fallback.font_family,
                ]
                .into_iter()
                .flatten()
                .any(|name| {
                    [
                        "Japan",
                        "Mincho",
                        "MS-Gothic",
                        "MS Gothic",
                        "NotoSansJP",
                        "NotoSerifJP",
                        "Hiragino",
                    ]
                    .iter()
                    .any(|part| name.contains(part))
                });
            if japanese {
                let data: hayro::hayro_interpret::font::FontData =
                    Arc::new(kagi_ui_core::fonts::japanese_font_data(
                        fallback.is_bold || fallback.font_weight >= 600,
                    ));
                return Some((data, 0));
            }
        }
        standard(query)
    });
    let partial = Arc::new(AtomicBool::new(false));
    let warning = partial.clone();
    settings.warning_sink = Arc::new(move |_| {
        warning.store(true, Ordering::Relaxed);
    });
    // RenderCache contains Rc/RefCell: construct, use and drop on this thread.
    let cache = hayro::RenderCache::new();
    let pixmap = hayro::render(
        page,
        &cache,
        &settings,
        &Default::default(),
        &hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let mut bytes = pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bytes).ok_or(PdfError::Dimensions)?;
    // Pinned GPUI caf5007 takes image::Frame and consumes BGRA pixels directly.
    Ok((
        Arc::new(gpui::RenderImage::new([image::Frame::new(buffer)])),
        partial.load(Ordering::Relaxed),
    ))
}

// Request identity is deliberately independent of page/zoom values: an ABA
// navigation or leaving and returning to the same cached tab is still newer.
struct RenderIdentity {
    generation: u64,
    path: PathBuf,
    request: u64,
    document: Arc<Pdf>,
}
impl RenderIdentity {
    fn matches(&self, generation: u64, path: Option<&Path>, pdf: &PdfPreview) -> bool {
        self.generation == generation
            && path == Some(self.path.as_path())
            && self.request == pdf.request
            && pdf
                .source
                .document
                .as_ref()
                .is_ok_and(|doc| Arc::ptr_eq(doc, &self.document))
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    pub(super) static FILE_CLASSIFICATION_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

impl EditorWorkspaceView {
    #[cfg(feature = "gui-e2e")]
    pub fn hold_next_file_classification_for_e2e(hold: gpui::Task<()>) {
        FILE_CLASSIFICATION_HOLD.with(|slot| {
            assert!(slot.borrow_mut().replace(hold).is_none());
        });
    }

    pub(super) fn is_pdf(&self) -> bool {
        // The accepted buffer owns its kind. Rename preserves dirty text even
        // when its new suffix is .pdf; pending reads use EditorLoading instead.
        self.content_pdf.is_some()
    }
    /// Current PDF presentation (including its pending/error state), not a
    /// historical presentation displayed over the retained current document.
    pub fn pdf_surface_visible(&self) -> bool {
        self.is_pdf()
            && !(self.right_tab == RightPaneTab::History && self.selected_history_commit.is_some())
    }
    pub(super) fn accept_pdf(&mut self, source: Option<PdfSource>) {
        let Some(source) = source else {
            self.content_pdf = None;
            return;
        };
        if self
            .content_pdf
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(&old.source.bytes, &source.bytes))
        {
            return;
        }
        let (page, zoom) = self
            .content_pdf
            .as_ref()
            .map_or((0, 1.), |old| (old.page, old.zoom));
        self.content_pdf = Some(PdfPreview::new(source, page, zoom));
        self.invalidate_pdf_request();
    }
    pub(super) fn invalidate_pdf_request(&mut self) {
        if let Some(pdf) = self.content_pdf.as_mut() {
            pdf.request = self.next_pdf_request;
            self.next_pdf_request = self.next_pdf_request.wrapping_add(1);
        }
    }
    pub(super) fn prepare_pdf_render(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.pdf_surface_visible() {
            return;
        }
        let Some(pdf) = self.content_pdf.as_mut() else {
            return;
        };
        let backing_scale = window.scale_factor();
        if pdf.backing_scale == Some(backing_scale) {
            return;
        }
        pdf.backing_scale = Some(backing_scale);
        if pdf.source.document.is_err() {
            return;
        }
        // Raster pixels follow this window's display; page geometry and user
        // zoom stay logical. Cached tabs also invalidate after a display move.
        pdf.image = None;
        pdf.error = None;
        pdf.partial = false;
        pdf.settled = false;
        self.invalidate_pdf_request();
        self.request_pdf_render(cx);
    }
    fn change_pdf(
        &mut self,
        page_delta: isize,
        zoom_factor: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(pdf) = self.content_pdf.as_mut() else {
            return;
        };
        let Ok(document) = &pdf.source.document else {
            return;
        };
        let page = pdf
            .page
            .saturating_add_signed(page_delta)
            .min(document.pages().len() - 1);
        let zoom = if reset {
            1.
        } else {
            (pdf.zoom * zoom_factor).clamp(MIN_ZOOM, MAX_ZOOM)
        };
        if page == pdf.page && zoom == pdf.zoom {
            return;
        }
        pdf.page = page;
        pdf.zoom = zoom;
        pdf.image = None;
        pdf.error = None;
        pdf.partial = false;
        pdf.settled = false;
        pdf.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.invalidate_pdf_request();
        self.request_pdf_render(cx);
        cx.notify();
    }
    pub(super) fn request_pdf_render(&mut self, cx: &mut Context<Self>) {
        if self.pdf_render_in_flight {
            return;
        }
        let Some(pdf) = self.content_pdf.as_ref().filter(|pdf| !pdf.settled) else {
            return;
        };
        let Ok(document) = &pdf.source.document else {
            return;
        };
        // Wait for the first visible render to supply the actual window scale.
        let Some(backing_scale) = pdf.backing_scale else {
            return;
        };
        let Some(path) = self.open_path.clone() else {
            return;
        };
        let (document, request, page, zoom, generation) = (
            document.clone(),
            pdf.request,
            pdf.page,
            pdf.zoom,
            self.buf_gen,
        );
        self.pdf_render_in_flight = true;
        let identity = RenderIdentity {
            generation,
            path,
            request,
            document: document.clone(),
        };
        let task =
            cx.background_spawn(async move { render_page(document, page, zoom, backing_scale) });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.pdf_render_in_flight = false;
                let generation = view.buf_gen;
                let path = view.open_path.as_deref();
                if let Some(pdf) = view
                    .content_pdf
                    .as_mut()
                    .filter(|pdf| identity.matches(generation, path, pdf))
                {
                    pdf.settled = true;
                    match result {
                        Ok((image, partial)) => {
                            pdf.image = Some(image);
                            pdf.partial = partial;
                        }
                        Err(error) => pdf.error = Some(error),
                    }
                }
                // One executing job plus the latest active desired request. Never
                // cancel a synchronous renderer and assume its CPU work stopped.
                view.request_pdf_render(cx);
                cx.notify();
            });
        })
        .detach();
    }
}

pub(super) fn status_message(message: Msg) -> impl IntoElement {
    div()
        .w_full()
        .min_w(px(0.))
        .p_3()
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .child(div().w_full().min_w(px(0.)).child(message.t()))
}

pub(super) fn render_preview(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    let pdf = view
        .content_pdf
        .as_ref()
        .expect("PDF presentation admission");
    let count = pdf
        .source
        .document
        .as_ref()
        .map_or(0, |doc| doc.pages().len());
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
            count == 0 || pdf.page == 0,
            -1,
            1.,
            false,
        ),
        (
            "ews-pdf-next",
            Msg::EditorPdfNext,
            Some(gpui_component::IconName::ChevronRight),
            count == 0 || pdf.page + 1 >= count,
            1,
            1.,
            false,
        ),
        (
            "ews-pdf-zoom-out",
            Msg::EditorPdfZoomOut,
            Some(gpui_component::IconName::Minus),
            count == 0 || pdf.zoom <= MIN_ZOOM,
            0,
            0.8,
            false,
        ),
        (
            "ews-pdf-zoom-in",
            Msg::EditorPdfZoomIn,
            Some(gpui_component::IconName::Plus),
            count == 0 || pdf.zoom >= MAX_ZOOM,
            0,
            1.25,
            false,
        ),
        (
            "ews-pdf-reset",
            Msg::EditorPdfResetZoom,
            None,
            count == 0 || pdf.zoom == 1.,
            0,
            1.,
            true,
        ),
    ] {
        let name: SharedString = msg.t().into();
        let button = Button::new(id)
            .ghost()
            .small()
            .compact()
            .when_some(icon, |button, icon| button.icon(icon))
            // Button::label would replace the localized accessible action name.
            .when(reset, |button| button.child("100%"))
            .disabled(disabled)
            .on_click(cx.listener(move |view, _, _, cx| view.change_pdf(delta, factor, reset, cx)));
        toolbar = toolbar.child(kagi_ui_core::tooltip::HoverTooltip::hover_tooltip(
            named_button::NamedButton(button)
                .role(gpui::Role::Button)
                .aria_label(name.clone()),
            (id, 0usize),
            name,
        ));
    }
    toolbar = toolbar.child(
        div()
            .w_full()
            .min_w(px(0.))
            .text_xs()
            .whitespace_normal()
            .child(format!(
                "{} {} / {} · {}% · {}",
                Msg::EditorPdfPage.t(),
                if count == 0 { 0 } else { pdf.page + 1 },
                count,
                (pdf.zoom * 100.).round() as u32,
                Msg::EditorPdfReadOnly.t()
            )),
    );
    let mut body = div()
        .id("ews-pdf-page-scroll")
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .overflow_scroll()
        .track_scroll(&pdf.scroll)
        .p_4()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, window, cx| view.root_focus.focus(window, cx)),
        );
    if let Some(error) = pdf.error {
        body = body.child(status_message(error.message()));
    } else if let Some(image) = pdf.image.clone() {
        if let Ok(document) = &pdf.source.document {
            let pages = document.pages();
            if let Some(page) = pages.get(pdf.page) {
                let (width, height) = page.render_dimensions();
                body = body.child(
                    gpui::img(image)
                        .w(px(width * pdf.zoom))
                        .h(px(height * pdf.zoom))
                        .flex_shrink_0(),
                );
            }
        }
    } else {
        body = body.child(status_message(Msg::EditorPdfLoading));
    }
    div()
        .id("ews-pdf")
        .key_context("EditorPdf")
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(toolbar)
        .when(pdf.partial, |el| {
            el.child(div().text_xs().px_2().child(Msg::EditorPdfPartial.t()))
        })
        .child(body)
        .into_any_element()
}

/// Native-test observation of the real document/raster and scroll owner.
#[cfg(feature = "gui-e2e")]
pub struct PdfProbe {
    pub page: usize,
    pub page_count: usize,
    pub zoom: f32,
    pub image: Option<Arc<gpui::RenderImage>>,
    pub error: Option<&'static str>,
    pub settled: bool,
    pub in_flight: bool,
    pub request: u64,
    pub scroll: gpui::ScrollHandle,
}

#[cfg(feature = "gui-e2e")]
impl EditorWorkspaceView {
    pub fn pdf_probe(&self) -> Option<PdfProbe> {
        let pdf = self.content_pdf.as_ref()?;
        Some(PdfProbe {
            page: pdf.page,
            page_count: pdf
                .source
                .document
                .as_ref()
                .map_or(0, |doc| doc.pages().len()),
            zoom: pdf.zoom,
            image: pdf.image.clone(),
            error: pdf.error.map(|error| error.message().t()),
            settled: pdf.settled,
            in_flight: self.pdf_render_in_flight,
            request: pdf.request,
            scroll: pdf.scroll.clone(),
        })
    }

    /// Batch rapid desired requests without advancing the native dispatcher.
    pub fn change_pdf_for_e2e(
        &mut self,
        delta: isize,
        factor: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        self.change_pdf(delta, factor, reset, cx);
    }
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
