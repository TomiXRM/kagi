//! Shared PDF source parsing and rasterization, used by Editor and commit previews.
//! hayro is not a hostile-document sandbox: output caps do not bound decoded
//! resources or synchronous interpreter time.
use super::*;
use hayro::hayro_interpret::{font::FontQuery, InterpreterSettings};
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use std::io::Read as _;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const MAX_SIDE: f64 = 4096.;
pub(super) const MAX_PIXELS: f64 = 16_000_000.;
// Manual controls stop here; one-shot fitting may use any finite positive zoom.
pub(super) const MIN_ZOOM: f32 = 0.25;
pub(super) const MAX_ZOOM: f32 = 4.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PdfError {
    Invalid,
    Password,
    Encryption,
    Dimensions,
    TooLarge,
}
impl PdfError {
    pub(super) fn message(self) -> Msg {
        match self {
            Self::Invalid => Msg::EditorPdfInvalid,
            Self::Password => Msg::EditorPdfPassword,
            Self::Encryption => Msg::EditorPdfEncryption,
            Self::Dimensions => Msg::EditorPdfDimensions,
            Self::TooLarge => Msg::EditorWorkspaceTooLarge,
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

/// Immutable, background-prepared PDF input, including visible parse errors.
///
/// Call `from_bytes` on a background worker, never from a render/UI callback.
#[derive(Clone)]
pub struct PdfSource {
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) document: Result<Arc<Pdf>, PdfError>,
    pub(super) dimensions: Arc<[(f32, f32)]>,
}
impl PdfSource {
    /// Inspect the already-parsed result without decoding or rasterizing again.
    pub fn is_valid(&self) -> bool {
        self.document.is_ok()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let too_large = bytes.len() as u64 > MAX_EDITOR_BYTES;
        let bytes = Arc::new(if too_large { Vec::new() } else { bytes });
        let document = if too_large {
            Err(PdfError::TooLarge)
        } else {
            Pdf::new(bytes.clone())
                .map_err(PdfError::from)
                .and_then(|pdf| {
                    if pdf.pages().is_empty() {
                        Err(PdfError::Invalid)
                    } else {
                        Ok(Arc::new(pdf))
                    }
                })
        };
        let dimensions: Arc<[(f32, f32)]> = document.as_ref().map_or_else(
            |_| Arc::from([]),
            |pdf| {
                pdf.pages()
                    .iter()
                    .map(|page| page.render_dimensions())
                    .collect()
            },
        );
        Self {
            bytes,
            document,
            dimensions,
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
                PdfSource::from_bytes(bytes)
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
pub(super) fn raster_scale(
    width: f32,
    height: f32,
    zoom: f32,
    backing_scale: f32,
) -> Result<f32, PdfError> {
    if !width.is_finite()
        || !height.is_finite()
        || !zoom.is_finite()
        || width <= 0.
        || height <= 0.
        || zoom <= 0.
        || zoom > MAX_ZOOM
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
    // Fitting can make a logical page subpixel. raster_scales gives each axis
    // at least one output pixel without inflating the logical layout.
    let (rw, rh) = (width * scale, height * scale);
    if !scale.is_finite()
        || rw <= 0.
        || rh <= 0.
        || f64::from(rw.floor()) > MAX_SIDE
        || f64::from(rh.floor()) > MAX_SIDE
        || f64::from(rw.floor()) * f64::from(rh.floor()) > MAX_PIXELS
    {
        return Err(PdfError::Dimensions);
    }
    Ok(scale)
}

pub(super) fn raster_scales(
    width: f32,
    height: f32,
    zoom: f32,
    backing_scale: f32,
    pixel_budget: usize,
) -> Result<(f32, f32), PdfError> {
    let maximum = raster_scale(width, height, zoom, backing_scale)?;
    if pixel_budget == 0 {
        return Err(PdfError::Dimensions);
    }
    let scale =
        maximum.min((pixel_budget as f64 / (f64::from(width) * f64::from(height))).sqrt() as f32);
    let mut raster_width = ((width * scale).floor() as usize).max(1);
    let mut raster_height = ((height * scale).floor() as usize).max(1);
    // Extreme aspect ratios can reach the one-pixel axis before exhausting the
    // other axis. Independent pixel densities keep that page visible too; the
    // image is still displayed at its unchanged logical page aspect ratio.
    if raster_width * raster_height > pixel_budget {
        if raster_width > raster_height {
            raster_width = (pixel_budget / raster_height).max(1);
        } else {
            raster_height = (pixel_budget / raster_width).max(1);
        }
    }
    let x_scale = (raster_width as f32 / width).next_up();
    let y_scale = (raster_height as f32 / height).next_up();
    let (actual_width, actual_height) = ((width * x_scale).floor(), (height * y_scale).floor());
    if actual_width < 1.
        || actual_height < 1.
        || f64::from(actual_width) > MAX_SIDE
        || f64::from(actual_height) > MAX_SIDE
        || actual_width as usize * actual_height as usize > pixel_budget
    {
        return Err(PdfError::Dimensions);
    }
    Ok((x_scale, y_scale))
}

#[cfg(test)]
fn render_page(
    document: Arc<Pdf>,
    page_index: usize,
    zoom: f32,
    backing_scale: f32,
) -> Result<(Arc<gpui::RenderImage>, bool), PdfError> {
    render_page_with_budget(
        document,
        page_index,
        zoom,
        backing_scale,
        MAX_PIXELS as usize,
    )
}

pub(super) fn render_page_with_budget(
    document: Arc<Pdf>,
    page_index: usize,
    zoom: f32,
    backing_scale: f32,
    pixel_budget: usize,
) -> Result<(Arc<gpui::RenderImage>, bool), PdfError> {
    let pages = document.pages();
    let page = pages.get(page_index).ok_or(PdfError::Invalid)?;
    let (width, height) = page.render_dimensions();
    let (x_scale, y_scale) = raster_scales(width, height, zoom, backing_scale, pixel_budget)?;
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
            x_scale,
            y_scale,
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

    pub(super) fn accept_pdf(&mut self, source: Option<PdfSource>, cx: &mut Context<Self>) {
        match (self.content_pdf.as_ref(), source) {
            (_, None) => self.content_pdf = None,
            (Some(viewer), Some(source)) => {
                viewer.update(cx, |viewer, cx| viewer.replace_source(source, cx));
            }
            (None, Some(source)) => {
                self.content_pdf = Some(cx.new(|cx| PdfViewer::new(source, cx)));
            }
        }
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_probe(&self, cx: &gpui::App) -> Option<PdfProbe> {
        self.content_pdf
            .as_ref()
            .map(|viewer| viewer.read(cx).pdf_probe())
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_viewer_for_e2e(&self) -> Option<Entity<PdfViewer>> {
        self.content_pdf.clone()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn change_pdf_for_e2e(
        &mut self,
        delta: isize,
        factor: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(viewer) = &self.content_pdf {
            viewer.update(cx, |viewer, cx| viewer.change_pdf(delta, factor, reset, cx));
        }
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
    pub source_id: usize,
    pub cached_pages: Vec<usize>,
    pub cached_bytes: usize,
    pub cache_entry_limit: usize,
    pub rendered_pages: Vec<usize>,
    pub raster_pixel_budget: usize,
    pub visible_pages: std::ops::Range<usize>,
    pub mounted_pages: std::ops::Range<usize>,
    pub page_sizes: Vec<(f32, f32)>,
    pub page_tops: Vec<f32>,
    pub raster_jobs: usize,
    pub raster_requests_started: usize,
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
