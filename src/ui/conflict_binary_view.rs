//! #321 — informative side-by-side viewer for binary / symlink / submodule
//! conflicts inside Conflict Mode.
//!
//! Backend resolution (take a side by raw OID) already works (#297); this module
//! only adds the VIEWER so the user can see what they are choosing. It sits
//! ABOVE the unchanged take-current / take-incoming buttons in `render_center`.
//!
//! Layer boundary (CLAUDE.md): all byte/blob access goes through
//! `Backend::conflict_side_bytes` / `conflict_side_blob_info` — no git2 here.
//!
//! Per side we render one of four modes:
//!   - **Image** (png/jpg/jpeg/gif/webp/bmp, within a size cap): the decoded
//!     image, current | incoming.
//!   - **Symlink**: the link target string (the blob content of a 120000 entry
//!     IS the target path).
//!   - **Submodule**: the side's commit OID (short).
//!   - **Binary** (and images beyond the cap): size + short OID, plus a shared
//!     "open both sides in external editor" action to compare.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{div, prelude::*, px, rgb, Context, Image, ImageFormat, SharedString};

use kagi_git::conflicts::{ConflictKind, SideLabels};
use kagi_git::resolution::SelectionSide;

use super::conflict_view::{ConflictMode, ConflictView};
use super::i18n::Msg;
use super::theme::{self, theme};

/// Decode budget: images larger than this are not decoded inline (shown as a
/// binary side with an "open externally" affordance instead). Keeps a huge blob
/// from being handed to the image decoder every session.
const IMAGE_SIZE_CAP: u64 = 8 * 1024 * 1024;
/// Max on-screen box for a previewed image side.
const IMAGE_MAX_PX: f32 = 260.;

// ────────────────────────────────────────────────────────────
// Pure helpers (unit-tested) — no gpui / git2 / I/O
// ────────────────────────────────────────────────────────────

/// The four viewer modes for a structurally-unmergeable conflict side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryViewMode {
    /// Render the side's blob as an image.
    Image,
    /// Show the side's link target string.
    Symlink,
    /// Show the side's commit OID.
    Submodule,
    /// Show size + OID; offer external-editor compare.
    Binary,
}

/// Map a supported image extension / magic-byte signature to a gpui
/// [`ImageFormat`]. Extension wins; magic bytes are the fallback when the path
/// has no (or a lying) extension. Returns `None` for anything not renderable.
pub fn image_format_for(path: &Path, magic: Option<&[u8]>) -> Option<ImageFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => return Some(ImageFormat::Png),
        Some("jpg") | Some("jpeg") => return Some(ImageFormat::Jpeg),
        Some("gif") => return Some(ImageFormat::Gif),
        Some("webp") => return Some(ImageFormat::Webp),
        Some("bmp") => return Some(ImageFormat::Bmp),
        _ => {}
    }
    image_format_from_magic(magic?)
}

/// Detect a supported image format from leading magic bytes.
fn image_format_from_magic(b: &[u8]) -> Option<ImageFormat> {
    if b.len() >= 8 && b[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        Some(ImageFormat::Png)
    } else if b.len() >= 3 && b[..3] == [0xff, 0xd8, 0xff] {
        Some(ImageFormat::Jpeg)
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if b.len() >= 6 && (&b[..6] == b"GIF87a" || &b[..6] == b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if b.len() >= 2 && &b[..2] == b"BM" {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

/// Decide the viewer mode for a conflict side. Submodule and Symlink follow the
/// conflict kind directly; a Binary conflict renders as an image only when the
/// blob is detected as one AND within the size cap, otherwise as a raw binary.
pub fn viewer_mode(kind: ConflictKind, is_image: bool, size: Option<u64>) -> BinaryViewMode {
    match kind {
        ConflictKind::Submodule => BinaryViewMode::Submodule,
        ConflictKind::Symlink => BinaryViewMode::Symlink,
        _ => {
            let within_cap = size.map(|s| s <= IMAGE_SIZE_CAP).unwrap_or(false);
            if is_image && within_cap {
                BinaryViewMode::Image
            } else {
                BinaryViewMode::Binary
            }
        }
    }
}

// ────────────────────────────────────────────────────────────
// Preview cache (content-addressed by blob OID) — resolve each side once
// ────────────────────────────────────────────────────────────

thread_local! {
    // ponytail: session-lifetime cache, never evicted. Keyed by blob OID (+
    // conflict kind) so it is content-addressed (no stale entries across
    // repos/sessions); a conflict session has a handful of files, so unbounded
    // growth is not a concern. Upgrade path: LRU if a huge multi-repo session
    // ever makes this matter.
    static SIDE_CACHE: RefCell<HashMap<String, SideData>> = RefCell::new(HashMap::new());
}

/// Cache-first lookup (#409): consult [`SIDE_CACHE`] BEFORE any repository
/// access. `build` (which opens the backend and reads the blob) runs only on a
/// miss and its result is cached; a `None` from `build` (backend open failure)
/// is not cached so a later frame can retry.
fn cached_or_build(key: &str, build: impl FnOnce() -> Option<SideData>) -> Option<SideData> {
    if let Some(hit) = SIDE_CACHE.with(|c| c.borrow().get(key).cloned()) {
        return Some(hit);
    }
    let data = build()?;
    SIDE_CACHE.with(|c| c.borrow_mut().insert(key.to_string(), data.clone()));
    Some(data)
}

/// Everything the viewer needs for one side, resolved once via the backend.
#[derive(Clone)]
struct SideData {
    present: bool,
    oid_short: String,
    size: Option<u64>,
    mode: BinaryViewMode,
    image: Option<Arc<Image>>,
    symlink_target: Option<String>,
}

impl SideData {
    fn absent() -> Self {
        SideData {
            present: false,
            oid_short: String::new(),
            size: None,
            mode: BinaryViewMode::Binary,
            image: None,
            symlink_target: None,
        }
    }
}

/// Build one side's viewer data via the backend (open once per render). Uses the
/// content-addressed image cache so a blob is only decoded once.
fn build_side(
    backend: &kagi_git::Backend,
    buffer: &kagi_git::resolution::ResolutionBuffer,
    path: &Path,
    side: SelectionSide,
    kind: ConflictKind,
) -> SideData {
    let Some(info) = backend.conflict_side_blob_info(buffer, path, side) else {
        return SideData::absent();
    };

    // Symlink: the blob IS the target path.
    if kind == ConflictKind::Symlink {
        let target = backend
            .conflict_side_bytes(buffer, path, side)
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        return SideData {
            present: true,
            oid_short: info.oid_short,
            size: info.size,
            mode: BinaryViewMode::Symlink,
            image: None,
            symlink_target: target,
        };
    }
    if kind == ConflictKind::Submodule {
        return SideData {
            present: true,
            oid_short: info.oid_short,
            size: info.size,
            mode: BinaryViewMode::Submodule,
            image: None,
            symlink_target: None,
        };
    }

    // Binary: within the size cap, fetch bytes and decide image-vs-raw by
    // extension AND magic bytes (catches an image with a wrong/absent
    // extension), decoding once per blob OID.
    let within_cap = info.size.map(|s| s <= IMAGE_SIZE_CAP).unwrap_or(false);
    let mut image = None;
    if within_cap {
        if let Some(bytes) = backend.conflict_side_bytes(buffer, path, side) {
            if let Some(fmt) = image_format_for(path, Some(&bytes)) {
                // #409: build_side only runs on a SIDE_CACHE miss, so this
                // decode (and the blob read above) happens once per blob.
                klog!(
                    "conflict-view: image decoded oid={} fmt={:?} src={}bytes",
                    info.oid_short,
                    fmt,
                    bytes.len()
                );
                image = Some(Arc::new(Image::from_bytes(fmt, bytes)));
            }
        }
    }
    let mode = viewer_mode(kind, image.is_some(), info.size);
    SideData {
        present: true,
        oid_short: info.oid_short,
        size: info.size,
        mode,
        image,
        symlink_target: None,
    }
}

// ────────────────────────────────────────────────────────────
// Render
// ────────────────────────────────────────────────────────────

/// The side-by-side viewer for a raw (binary / symlink / submodule) conflict.
/// Rendered above the unchanged take-current / take-incoming buttons.
pub fn render_raw_preview(
    mode: &ConflictMode,
    path: &Path,
    kind: ConflictKind,
    labels: &SideLabels,
    _cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    // #409: the cache key comes from the buffer alone (`side_raw_meta`, no
    // repository access) and SIDE_CACHE is consulted first — the backend is
    // opened, and blob bytes read, only when a side misses the cache. Before
    // this, every repaint on the one screen where the user types opened a
    // git2 Repository and copied both sides' full blobs.
    let mut backend: Option<kagi_git::Backend> = None;
    let mut side = |s: SelectionSide| -> Option<SideData> {
        let Some((oid, _)) = mode.buffer.side_raw_meta(path, s) else {
            return Some(SideData::absent());
        };
        cached_or_build(&format!("{oid}:{kind:?}"), || {
            if backend.is_none() {
                backend = kagi_git::Backend::open(mode.buffer.repo_path()).ok();
            }
            let b = backend.as_ref()?;
            Some(build_side(b, &mode.buffer, path, s, kind))
        })
    };
    let (current, incoming) = match (side(SelectionSide::Current), side(SelectionSide::Incoming)) {
        (Some(c), Some(i)) => (c, i),
        _ => return raw_error_box(),
    };

    // Any non-image binary side offers the external-compare action.
    let show_external =
        current.mode == BinaryViewMode::Binary || incoming.mode == BinaryViewMode::Binary;

    let mut col = div()
        .id("conflict-raw-preview")
        .flex()
        .flex_col()
        .flex_grow(1.)
        .w_full()
        .overflow_y_scroll()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .gap(theme::scaled_px(8.))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(theme::scaled_px(12.))
                .child(side_column(
                    &format!("{} · {}", Msg::ConflictRoleCurrent.t(), labels.current.name),
                    &current,
                ))
                .child(side_column(
                    &format!(
                        "{} · {}",
                        Msg::ConflictRoleIncoming.t(),
                        labels.incoming.name
                    ),
                    &incoming,
                )),
        );

    if show_external {
        let p = path.to_path_buf();
        use gpui_component::Sizable as _;
        col = col.child(
            super::button_style::KagiButton::accent(
                SharedString::from("conflict-open-both-external"),
                SharedString::from(Msg::ConflictOpenBothExternal.t()),
                theme().text_sub,
                _cx,
            )
            .small()
            .on_click(_cx.listener(move |view: &mut ConflictView, _e, _w, cx| {
                view.open_raw_sides_external(&p, cx);
            })),
        );
    }

    col.into_any_element()
}

/// One side's column: header + mode-specific body.
fn side_column(header: &str, data: &SideData) -> gpui::AnyElement {
    let body: gpui::AnyElement = if !data.present {
        muted(Msg::ConflictBinaryNoPreview.t())
    } else {
        match data.mode {
            BinaryViewMode::Image => match &data.image {
                // Mirror the WORKING avatar path (inspector.rs:558): an
                // EXPLICIT-pixel-size, flex_shrink_0 container + img.size_full().
                // `w_full` here resolves to 0 inside the flex_basis(0)/min_w(0)
                // column, so the image box collapsed and painted nothing (#362).
                // object_fit(Contain) keeps aspect ratio inside the fixed box.
                Some(img) => div()
                    .w(px(IMAGE_MAX_PX))
                    .h(px(IMAGE_MAX_PX))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(
                        gpui::img(gpui::ImageSource::Image(img.clone()))
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element(),
                None => muted(Msg::ConflictImageTooLarge.t()),
            },
            BinaryViewMode::Symlink => div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(label_line(Msg::ConflictSymlinkTarget.t()))
                .child(mono(data.symlink_target.clone().unwrap_or_default()))
                .into_any_element(),
            BinaryViewMode::Submodule => div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(label_line(Msg::ConflictSubmoduleCommit.t()))
                .child(mono(data.oid_short.clone()))
                .into_any_element(),
            BinaryViewMode::Binary => div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(mono(format!(
                    "{} · {}",
                    size_text(data.size),
                    data.oid_short
                )))
                .child(muted(Msg::ConflictBinaryCompareHint.t()))
                .into_any_element(),
        }
    };

    div()
        .flex()
        .flex_col()
        .flex_grow(1.)
        .flex_basis(px(0.))
        .min_w(px(0.))
        .gap(px(4.))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(header.to_string())),
        )
        .child(body)
        .into_any_element()
}

fn size_text(size: Option<u64>) -> String {
    match size {
        Some(n) => format!("{} B", n),
        None => "—".to_string(),
    }
}

fn label_line(text: &str) -> gpui::AnyElement {
    div()
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(theme().text_label))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

fn mono(text: String) -> gpui::AnyElement {
    div()
        .text_size(theme::scaled_px(12.))
        .text_color(rgb(theme().text_main))
        .child(SharedString::from(text))
        .into_any_element()
}

fn muted(text: &str) -> gpui::AnyElement {
    div()
        .text_size(theme::scaled_px(12.))
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

fn raw_error_box() -> gpui::AnyElement {
    div()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .child(muted(Msg::ConflictBinaryNoPreview.t()))
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// ConflictView action: materialize both sides + open externally
// ────────────────────────────────────────────────────────────

impl ConflictView {
    /// Materialize each side's blob to a temp file and open both in the external
    /// editor, so the user can compare non-renderable binaries (#321). Never
    /// blocks Conflict Mode; failures surface as a toast.
    pub(crate) fn open_raw_sides_external(&mut self, path: &Path, cx: &mut Context<Self>) {
        let repo_path = self.repo_path.clone();
        let backend = match kagi_git::Backend::open(&repo_path) {
            Ok(b) => b,
            Err(_) => return,
        };
        let buffer = match backend.resolution_buffer_from_repo_with_autosave() {
            Ok(b) => b,
            Err(_) => return,
        };

        let stem = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        let mut temps: Vec<PathBuf> = Vec::new();
        for (side, tag) in [
            (SelectionSide::Current, "current"),
            (SelectionSide::Incoming, "incoming"),
        ] {
            if let Some(bytes) = backend.conflict_side_bytes(&buffer, path, side) {
                let tmp = std::env::temp_dir().join(format!("kagi-conflict-{}-{}", tag, stem));
                if std::fs::write(&tmp, &bytes).is_ok() {
                    temps.push(tmp);
                }
            }
        }

        if temps.is_empty() {
            return;
        }
        // Defer to the parent: `open_files_external` (settings + toast) lives on
        // KagiApp; calling it directly from a leased listener would re-enter.
        let weak_app = self.app.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.open_files_external(&temps, cx);
            });
        })
        .detach();
    }
}

// ────────────────────────────────────────────────────────────
// Tests — pure helpers only (the render path needs a gpui window; see #321
// acceptance: GUI-verify the rendering).
// ────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    /// MUTATION GUARD (#409): the cache is consulted BEFORE the expensive
    /// build. On a hit the builder must not run at all (it panics here); on a
    /// miss it runs exactly once and the result is cached for the next frame.
    #[test]
    fn side_cache_consulted_before_build() {
        SIDE_CACHE.with(|c| {
            c.borrow_mut()
                .insert("hit-oid:Binary".into(), SideData::absent())
        });
        let hit = cached_or_build("hit-oid:Binary", || {
            panic!("cache hit must not open the backend / read the blob")
        });
        assert!(hit.is_some());

        let mut builds = 0;
        for _frame in 0..3 {
            let _ = cached_or_build("miss-oid:Binary", || {
                builds += 1;
                Some(SideData::absent())
            });
        }
        assert_eq!(builds, 1, "blob must be read once, not per frame");
    }

    #[test]
    fn image_detected_by_extension() {
        let is_img = |p: &str| image_format_for(Path::new(p), None).is_some();
        assert!(is_img("logo.PNG"));
        assert!(is_img("a.jpeg"));
        assert!(is_img("a.jpg"));
        assert!(is_img("a.gif"));
        assert!(is_img("a.webp"));
        assert!(is_img("a.bmp"));
        assert!(!is_img("a.bin"));
        assert!(!is_img("noext"));
    }

    #[test]
    fn image_detected_by_magic_when_extension_missing_or_wrong() {
        // No/renamed extension but PNG magic → still an image.
        assert_eq!(
            image_format_for(Path::new("blob.bin"), Some(&PNG_MAGIC)),
            Some(ImageFormat::Png)
        );
        // Non-image magic → not an image.
        assert!(image_format_for(Path::new("blob.bin"), Some(&[0u8, 1, 2, 3])).is_none());
    }

    #[test]
    fn viewer_mode_follows_kind() {
        assert_eq!(
            viewer_mode(ConflictKind::Submodule, false, None),
            BinaryViewMode::Submodule
        );
        assert_eq!(
            viewer_mode(ConflictKind::Symlink, false, Some(10)),
            BinaryViewMode::Symlink
        );
    }

    #[test]
    fn viewer_mode_image_only_when_image_and_within_cap() {
        assert_eq!(
            viewer_mode(ConflictKind::Binary, true, Some(1024)),
            BinaryViewMode::Image
        );
        // Image but over the cap → binary (external-compare) fallback.
        assert_eq!(
            viewer_mode(ConflictKind::Binary, true, Some(IMAGE_SIZE_CAP + 1)),
            BinaryViewMode::Binary
        );
        // Not an image → binary.
        assert_eq!(
            viewer_mode(ConflictKind::Binary, false, Some(1024)),
            BinaryViewMode::Binary
        );
    }
}
