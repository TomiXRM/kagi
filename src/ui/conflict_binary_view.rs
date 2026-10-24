//! #321 — informative side-by-side viewer for binary / symlink / submodule
//! conflicts inside Conflict Mode.
//!
//! Backend resolution (take a side by raw OID) stays authoritative. ConflictView
//! retains an accepted, off-thread prepared pair; rendering never opens Backend.
//! Choice controls and the existing side layout remain unchanged in ADR-0228 step ①.
//!
//! Per side we render one of four modes:
//!   - **Image** (png/jpg/jpeg/gif/webp/bmp, within a size cap): the decoded
//!     image, current | incoming.
//!   - **Symlink**: the link target string (the blob content of a 120000 entry
//!     IS the target path).
//!   - **Submodule**: the side's commit OID (short).
//!   - **Binary** (and images beyond the cap): size + short OID, plus a shared
//!     "open both sides in external editor" action to compare.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{div, prelude::*, px, rgb, Context, Image, SharedString};

use kagi_git::conflicts::{ConflictKind, SideLabels};
use kagi_git::resolution::SelectionSide;

use super::conflict_view::ConflictView;
use super::i18n::Msg;
use super::theme::{self, theme};

#[cfg(feature = "gui-e2e")]
mod test_support;

/// Decode budget: images larger than this are not decoded inline (shown as a
/// binary side with an "open externally" affordance instead). Keeps a huge blob
/// from being handed to the image decoder every session.
pub(crate) const IMAGE_SIZE_CAP: u64 = 8 * 1024 * 1024;
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

/// Everything the viewer needs for one side, resolved once via the backend.
#[derive(Clone)]
pub(crate) struct SideData {
    pub(crate) present: bool,
    pub(crate) oid_short: String,
    pub(crate) size: Option<u64>,
    pub(crate) mode: BinaryViewMode,
    pub(crate) image: Option<Arc<Image>>,
    pub(crate) symlink_target: Option<String>,
    /// A present side can fail preparation; it must never become `Absent`.
    pub(crate) error: Option<String>,
}

impl SideData {
    pub(crate) fn absent() -> Self {
        SideData {
            present: false,
            oid_short: String::new(),
            size: None,
            mode: BinaryViewMode::Binary,
            image: None,
            symlink_target: None,
            error: None,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) enum PreviewState {
    #[default]
    Loading,
    Ready(Arc<[SideData; 2]>),
    Failed(Arc<str>),
}

/// One attachment's accepted presentation and cancellable preparation lifetime.
#[derive(Default)]
pub(crate) struct RawPreview {
    pub(crate) state: PreviewState,
    key: Option<PreviewKey>,
    task: Option<gpui::Task<()>>,
    generation: u64,
    failed: Option<(PreviewKey, u8)>,
}

impl RawPreview {
    pub(crate) fn retire(&mut self) {
        self.generation += 1;
        self.task = None;
        self.key = None;
        self.failed = None;
        self.state = PreviewState::Loading;
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreviewKey {
    path: PathBuf,
    revision: kagi_domain::conflict_family::ConflictRevision,
    sides: [Option<(String, u32)>; 2],
    kind: ConflictKind,
}

impl ConflictView {
    pub(crate) fn retire_raw_preview(&mut self) {
        self.raw_preview.retire();
    }

    fn selected_raw_key(&self) -> Option<PreviewKey> {
        let mode = self.mode.as_ref()?;
        let file = mode.session.files.get(mode.selected_file?)?;
        if self.selected_saved.is_some() || !file.kind.is_raw() {
            return None;
        }
        Some(PreviewKey {
            path: file.path.clone(),
            revision: mode.revision.clone(),
            sides: [SelectionSide::Current, SelectionSide::Incoming]
                .map(|side| mode.buffer.side_raw_meta(&file.path, side)),
            kind: file.kind,
        })
    }

    fn raw_preview_matches(&self, key: &PreviewKey) -> bool {
        self.selected_saved.is_none()
            && self.mode.as_ref().is_some_and(|mode| {
                mode.revision == key.revision
                    && mode
                        .selected_file
                        .and_then(|index| mode.session.files.get(index))
                        .is_some_and(|file| file.path == key.path && file.kind == key.kind)
            })
    }

    /// Frame admission only freezes descriptors; all Backend access is in the worker.
    pub(crate) fn sync_raw_preview(&mut self, cx: &mut Context<Self>) {
        if self
            .raw_preview
            .key
            .as_ref()
            .is_some_and(|key| self.raw_preview_matches(key))
        {
            return;
        }
        // At most one retry per accepted observation/selection lifetime. Failed
        // requests relinquish their active key, but a repaint cannot hot-loop.
        if self
            .raw_preview
            .failed
            .as_ref()
            .is_some_and(|(key, failures)| *failures >= 2 && self.raw_preview_matches(key))
        {
            return;
        }
        let key = self.selected_raw_key();
        let failures = self
            .raw_preview
            .failed
            .as_ref()
            .map_or(0, |(failed, count)| {
                if Some(failed) == key.as_ref() {
                    *count
                } else {
                    0
                }
            });
        self.retire_raw_preview();
        let Some(key) = key else { return };
        self.raw_preview.key = Some(key.clone());
        let generation = self.raw_preview.generation;
        let repo = self.repo_path.clone();
        let owner = self.owner.clone();
        let input = key.clone();
        let task = cx.background_spawn(async move {
            use super::main_diff_pane::preparation::{
                prepare_side_pair, MediaSide, PreparationPolicy,
            };
            use kagi_git::backend::file_reads::FileReadSource;
            let backend = kagi_git::Backend::open(&repo)?;
            let sides = input.sides.each_ref().map(|side| match side {
                Some((oid, mode)) => MediaSide::File(
                    FileReadSource::ConflictStage { oid, mode: *mode },
                    &input.path,
                ),
                None => MediaSide::Absent,
            });
            Ok::<_, kagi_git::GitError>(
                prepare_side_pair(&backend, sides, PreparationPolicy::Conflict(input.kind))
                    .raw
                    .expect("conflict preparation returns a raw pair"),
            )
        });
        #[cfg(feature = "gui-e2e")]
        let hold = test_support::take_hold();
        self.raw_preview.task = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = &hold {
                hold.reached();
            }
            #[cfg(feature = "gui-e2e")]
            let completion = match hold {
                Some(hold) => Some(hold.wait().await),
                None => None,
            };
            let _ = view.update(cx, |view, cx| {
                let accepted = view.owner == owner
                    && view.raw_preview.generation == generation
                    && view.raw_preview.key.as_ref() == Some(&key)
                    && view.raw_preview_matches(&key);
                #[cfg(feature = "gui-e2e")]
                test_support::record_completion(completion, accepted);
                if !accepted {
                    return;
                }
                match result {
                    Ok(pair) => view.raw_preview.state = PreviewState::Ready(Arc::new(pair)),
                    Err(error) => {
                        view.raw_preview.key = None;
                        view.raw_preview.failed = Some((key, failures + 1));
                        view.raw_preview.state = PreviewState::Failed(error.to_string().into());
                    }
                }
                cx.notify();
            });
        }));
    }
}

// ────────────────────────────────────────────────────────────
// Render
// ────────────────────────────────────────────────────────────

/// The side-by-side viewer for a raw (binary / symlink / submodule) conflict.
/// Rendered below the unchanged take-current / take-incoming buttons.
pub(crate) fn render_raw_preview(
    state: &PreviewState,
    path: &Path,
    labels: &SideLabels,
    scroll: &gpui::ScrollHandle,
    _cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let (pair, failure) = match state {
        PreviewState::Loading => (None, None),
        PreviewState::Ready(pair) => (Some(pair.as_ref()), None),
        PreviewState::Failed(error) => (None, Some(error.as_ref())),
    };
    let current = pair.map(|pair| &pair[0]);
    let incoming = pair.map(|pair| &pair[1]);
    // Loading retains the two headers without claiming absence or failure.
    let image_layout = pair.is_none()
        || pair.is_some_and(|pair| pair.iter().any(|side| side.mode == BinaryViewMode::Image));
    let show_external =
        pair.is_some_and(|pair| pair.iter().any(|side| side.mode == BinaryViewMode::Binary));

    #[cfg(feature = "gui-e2e")]
    super::e2e::diff_presentation::record_conflict_scroll(scroll.clone());
    let mut col = div()
        .id("conflict-raw-preview")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .self_stretch()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .gap(theme::scaled_px(8.))
        .child(
            div()
                .flex()
                .flex_row()
                .when(image_layout, |row| row.flex_1().min_h(px(0.)))
                .self_stretch()
                .justify_center()
                .gap(theme::scaled_px(12.))
                .child(side_column(
                    &format!("{} · {}", Msg::ConflictRoleCurrent.t(), labels.current.name),
                    current,
                    failure,
                    "conflict-image-current",
                ))
                .child(side_column(
                    &format!(
                        "{} · {}",
                        Msg::ConflictRoleIncoming.t(),
                        labels.incoming.name
                    ),
                    incoming,
                    failure,
                    "conflict-image-incoming",
                )),
        );

    if show_external {
        col = col.child(super::e2e::measure_control(
            "conflict-open-both-external",
            external_compare_button(path, _cx),
        ));
    }

    // Padding belongs to the bounded pane, not the scroll content: fractional
    // zoom padding must not become a spurious horizontal scroll extent.
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .child(super::e2e::measure_inside("conflict-image-pane"))
        .child(col)
        .into_any_element()
}

fn side_body(data: &SideData, probe: &'static str) -> gpui::AnyElement {
    if let Some(error) = &data.error {
        return read_error(error);
    }
    if !data.present {
        muted(Msg::ConflictBinaryNoPreview.t())
    } else {
        match data.mode {
            BinaryViewMode::Image => match &data.image {
                // Match the bounded absolute-image layout in diff_state: image
                // intrinsic dimensions must not contribute to the flex minimum.
                Some(img) => div()
                    .w_full()
                    .relative()
                    .child(super::e2e::measure_inside(probe))
                    .aspect_ratio(1.)
                    .flex_shrink(1.)
                    .min_h(px(0.))
                    .max_h(theme::scaled_px(IMAGE_MAX_PX))
                    .overflow_hidden()
                    .child(
                        gpui::img(gpui::ImageSource::Image(img.clone()))
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element(),
                None => muted(Msg::ConflictImageTooLarge.t()),
            },
            BinaryViewMode::Symlink => div()
                .flex()
                .flex_col()
                .gap(theme::scaled_px(2.))
                .child(label_line(Msg::ConflictSymlinkTarget.t()))
                .child(mono(data.symlink_target.clone().unwrap_or_default()))
                .into_any_element(),
            BinaryViewMode::Submodule => div()
                .flex()
                .flex_col()
                .gap(theme::scaled_px(2.))
                .child(label_line(Msg::ConflictSubmoduleCommit.t()))
                .child(mono(data.oid_short.clone()))
                .into_any_element(),
            BinaryViewMode::Binary => div()
                .relative()
                .child(super::e2e::measure_inside(
                    if probe == "conflict-image-current" {
                        "conflict-binary-current-body"
                    } else {
                        "conflict-binary-incoming-body"
                    },
                ))
                .flex()
                .flex_col()
                .gap(theme::scaled_px(2.))
                .child(mono(format!(
                    "{} · {}",
                    size_text(data.size),
                    data.oid_short
                )))
                .child(super::render_helpers::diff_state::caption(
                    Msg::DiffBinaryNoText,
                ))
                .into_any_element(),
        }
    }
}

/// One side's column: header + mode-specific body.
fn side_column(
    header: &str,
    data: Option<&SideData>,
    failure: Option<&str>,
    probe: &'static str,
) -> gpui::AnyElement {
    let body = match (data, failure) {
        (Some(data), _) => side_body(data, probe),
        (None, Some(error)) => read_error(error),
        (None, None) => div().into_any_element(),
    };

    // Keep adjacent preferred-width columns, shrinking both equally at high
    // zoom rather than allowing the pair to overflow the available pane.
    div()
        .flex()
        .relative()
        .child(super::e2e::measure_inside(
            if probe == "conflict-image-current" {
                "conflict-image-current-column"
            } else {
                "conflict-image-incoming-column"
            },
        ))
        .flex_col()
        .w(theme::scaled_px(IMAGE_MAX_PX))
        .flex_shrink(1.)
        .when(
            data.is_none_or(|data| data.mode == BinaryViewMode::Image),
            |column| column.h_full(),
        )
        .min_h(px(0.))
        .min_w(px(0.))
        .gap(theme::scaled_px(4.))
        .child(
            div()
                .relative()
                .child(super::e2e::measure_inside(
                    if probe == "conflict-image-current" {
                        "conflict-image-current-header"
                    } else {
                        "conflict-image-incoming-header"
                    },
                ))
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(header.to_string())),
        )
        .child(body)
        .when_some(
            data.and_then(|data| data.image.as_ref()),
            |column, image| {
                let data = data.expect("image belongs to a prepared side");
                column.child(ImageMetadata {
                    image: image.clone(),
                    size: data.size,
                    probe,
                })
            },
        )
        .into_any_element()
}

#[derive(gpui::IntoElement)]
struct ImageMetadata {
    image: Arc<Image>,
    size: Option<u64>,
    probe: &'static str,
}

impl gpui::RenderOnce for ImageMetadata {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let dimensions = self
            .image
            .use_render_image(window, cx)
            .map(|image| image.size(0));
        let mut text = format!("{}: {}", Msg::ImageFileSize.t(), size_text(self.size));
        if let Some(size) = dimensions {
            use std::fmt::Write as _;
            let _ = write!(
                text,
                " · {}: {} × {} px",
                Msg::ImageDimensions.t(),
                size.width.0,
                size.height.0
            );
        }
        #[cfg(feature = "gui-e2e")]
        super::e2e::diff_presentation::record_image_caption(self.probe, &text);
        div()
            .relative()
            .child(super::e2e::measure_inside(
                if self.probe == "conflict-image-current" {
                    "conflict-image-current-caption"
                } else {
                    "conflict-image-incoming-caption"
                },
            ))
            .text_size(theme::scaled_px(11.))
            .text_color(rgb(theme().text_muted))
            .child(text)
    }
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

fn read_error(error: &str) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .child(muted(Msg::DiffPdfReadFailed.t()))
        .child(muted(error))
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
        let Some(mode) = self.mode.as_ref() else {
            return;
        };
        let sides = [SelectionSide::Current, SelectionSide::Incoming]
            .map(|side| mode.buffer.side_raw_meta(path, side));
        let path = path.to_path_buf();
        let task = cx.background_spawn(async move {
            use kagi_git::backend::file_reads::FileReadSource;
            let backend = kagi_git::Backend::open(&repo_path)?;
            let stem = path.file_name().unwrap_or_default().to_string_lossy();
            let mut temps = Vec::new();
            for (side, tag) in sides.into_iter().zip(["current", "incoming"]) {
                let Some((oid, mode)) = side else { continue };
                // External materialization is not an inline preview: preserve its
                // existing unrestricted transfer, on a worker rather than render.
                if let Some(bytes) = backend.file_bytes(
                    FileReadSource::ConflictStage { oid: &oid, mode },
                    &path,
                    usize::MAX,
                )? {
                    let tmp = std::env::temp_dir().join(format!("kagi-conflict-{}-{}", tag, stem));
                    std::fs::write(&tmp, bytes)
                        .map_err(|error| kagi_git::GitError::Other(error.to_string()))?;
                    temps.push(tmp);
                }
            }
            Ok::<_, kagi_git::GitError>(temps)
        });
        let weak_app = self.app.clone();
        cx.spawn(async move |view, acx| {
            let temps = match task.await {
                Ok(temps) => temps,
                Err(error) => {
                    let _ = view.update(acx, |view, cx| {
                        view.marshal_error_toast(error.to_string(), cx)
                    });
                    return;
                }
            };
            if temps.is_empty() {
                return;
            }
            let _ = weak_app.update(acx, |app, cx| {
                app.open_files_external(&temps, cx);
            });
        })
        .detach();
    }
}

fn external_compare_button(path: &Path, cx: &mut Context<ConflictView>) -> impl IntoElement {
    let path = path.to_path_buf();
    use gpui_component::Sizable as _;
    super::button_style::KagiButton::styled(
        SharedString::from("conflict-open-both-external"),
        SharedString::from(Msg::ConflictOpenBothExternal.t()),
        super::button_style::ButtonRole::Neutral,
        cx,
    )
    .small()
    .on_click(cx.listener(move |view: &mut ConflictView, _e, _w, cx| {
        view.open_raw_sides_external(&path, cx);
    }))
}

// ────────────────────────────────────────────────────────────
// Tests — pure helpers only (the render path needs a gpui window; see #321
// acceptance: GUI-verify the rendering).
// ────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

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
