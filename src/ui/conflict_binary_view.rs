//! Conflict-owned admission, adoption and lifetime for the shared side-pair viewer.
//! Preparation and content rendering are shared with diff. Decisions and Save
//! remain authoritative in ResolutionBuffer; preview bytes never enter Save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use gpui_component::Disableable as _;

use kagi_git::conflicts::{ConflictKind, SideLabels};
use kagi_git::resolution::SelectionSide;

use super::conflict_view::ConflictView;
use super::diff_view::pdf::{DiffPdfViewers, PreparedPdfPair};
use super::diff_view::{SideContent, SideHeader, SidePair};
use super::i18n::Msg;
use super::theme::{self, theme};

#[cfg(feature = "gui-e2e")]
mod test_support;

#[derive(Clone, Default)]
pub(crate) enum PreviewState {
    #[default]
    Loading,
    Ready(Arc<[SideContent; 2]>),
    Failed(Arc<str>),
}

/// One attachment's accepted presentation and cancellable preparation lifetime.
#[derive(Default)]
pub(crate) struct RawPreview {
    pub(crate) state: PreviewState,
    pub(crate) pdfs: Option<DiffPdfViewers>,
    key: Option<PreviewKey>,
    task: Option<gpui::Task<()>>,
    generation: u64,
    failed: Option<(PreviewKey, u8)>,
}

impl RawPreview {
    pub(crate) fn retire(&mut self, cx: &mut gpui::App) {
        if let Some(pdfs) = self.pdfs.take() {
            pdfs.retire_detached();
        }
        self.generation += 1;
        self.task = None;
        self.key = None;
        self.failed = None;
        self.replace_state(PreviewState::Loading, cx);
    }

    fn replace_state(&mut self, state: PreviewState, cx: &mut gpui::App) {
        if let PreviewState::Ready(pair) = std::mem::replace(&mut self.state, state) {
            super::diff_view::retire_side_images(pair.iter(), cx);
        }
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
    pub(crate) fn retire_raw_preview(&mut self, cx: &mut gpui::App) {
        self.raw_preview.retire(cx);
    }

    pub(crate) fn focused_pdf_body(
        &self,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> Option<gpui::FocusHandle> {
        self.raw_preview.pdfs.as_ref()?.focused_body(window, cx)
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_viewer_for_e2e(
        &self,
        old: bool,
        cx: &gpui::App,
    ) -> Option<gpui::Entity<kagi_ui_editor::PdfViewer>> {
        self.raw_preview.pdfs.as_ref()?.viewer(old, cx)
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
        self.retire_raw_preview(cx);
        let Some(key) = key else { return };
        self.raw_preview.key = Some(key.clone());
        let generation = self.raw_preview.generation;
        let repo = self.repo_path.clone();
        let owner = self.owner.clone();
        let input = key.clone();
        let task = cx.background_spawn(async move {
            use super::main_diff_pane::preparation::{prepare_content_pair, MediaSide};
            use kagi_git::backend::file_reads::FileReadSource;
            let backend = kagi_git::Backend::open(&repo)?;
            let sides = input.sides.each_ref().map(|side| match side {
                Some((oid, mode)) => MediaSide::File(
                    FileReadSource::ConflictStage { oid, mode: *mode },
                    &input.path,
                ),
                None => MediaSide::Absent,
            });
            Ok::<_, kagi_git::GitError>(prepare_content_pair(&backend, sides))
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
                    Ok(pair) => {
                        if pair
                            .iter()
                            .any(|side| matches!(side, SideContent::Pdf { .. }))
                        {
                            view.raw_preview.pdfs = Some(DiffPdfViewers::new_synchronized(
                                PreparedPdfPair {
                                    old: pair[0].pdf_source(),
                                    new: pair[1].pdf_source(),
                                },
                                cx,
                            ));
                        }
                        view.raw_preview
                            .replace_state(PreviewState::Ready(Arc::new(pair)), cx);
                    }
                    Err(error) => {
                        view.raw_preview.key = None;
                        view.raw_preview.failed = Some((key, failures + 1));
                        view.raw_preview
                            .replace_state(PreviewState::Failed(error.to_string().into()), cx);
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

/// Side identities stay above the persistent notification lane and scroll body.
pub(crate) fn render_raw_headers(labels: &SideLabels) -> gpui::AnyElement {
    let header = |label: String, accent, probe| {
        super::render_helpers::diff_state::side_header(label.into(), Some(accent))
            .relative()
            .flex_1()
            .min_w(px(0.))
            .child(super::e2e::measure_inside(probe))
    };
    div()
        .relative()
        .flex()
        .flex_shrink_0()
        .child(super::e2e::measure_inside("conflict-raw-headers"))
        .child(header(
            format!("{} — {}", labels.current.name, Msg::EditorCurrentSide.t()),
            theme().color_branch,
            "conflict-image-current-header",
        ))
        .child(div().w(px(1.)).flex_shrink_0().bg(rgb(theme().surface)))
        .child(header(
            format!("{} — {}", labels.incoming.name, Msg::EditorIncomingSide.t()),
            theme().color_remote,
            "conflict-image-incoming-header",
        ))
        .into_any_element()
}

/// Source presence determines whether adoption or the deletion confirmation owns
/// a side's action. The latter is wired through #1734's frozen intent boundary.
#[derive(Clone, Copy)]
enum RawSideTarget {
    Adopt(SelectionSide),
    Deleted,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_raw_preview(
    state: &PreviewState,
    mode: &super::conflict_view::ConflictMode,
    path: &Path,
    labels: &SideLabels,
    scroll: &gpui::ScrollHandle,
    pdfs: Option<&DiffPdfViewers>,
    writer_busy: bool,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let contents = match state {
        PreviewState::Loading => [SideContent::Loading, SideContent::Loading],
        PreviewState::Ready(pair) => pair.as_ref().clone(),
        PreviewState::Failed(error) => [
            SideContent::failed(SharedString::from(error.to_string())),
            SideContent::failed(SharedString::from(error.to_string())),
        ],
    };
    let show_external = matches!(state, PreviewState::Ready(_))
        && contents.iter().any(SideContent::needs_external_compare);
    let [left, right] = contents;
    let pair = SidePair {
        left,
        right,
        headers: [
            SideHeader {
                label: format!("{} — {}", labels.current.name, Msg::EditorCurrentSide.t()).into(),
                accent: Some(theme().color_branch),
                id: "conflict-raw-current",
                absent: Msg::ConflictSideDeleted,
            },
            SideHeader {
                label: format!("{} — {}", labels.incoming.name, Msg::EditorIncomingSide.t()).into(),
                accent: Some(theme().color_remote),
                id: "conflict-raw-incoming",
                absent: Msg::ConflictSideDeleted,
            },
        ],
    };
    let actions = [SelectionSide::Current, SelectionSide::Incoming]
        .map(|side| raw_action_row(mode, path, side, writer_busy, cx));
    #[cfg(feature = "gui-e2e")]
    super::e2e::diff_presentation::record_conflict_scroll(scroll.clone());
    div()
        .id("conflict-raw-preview")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .overflow_hidden()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .child(super::e2e::measure_inside("conflict-image-pane"))
        .child(super::render_helpers::diff_state::side_pair(
            pair,
            actions,
            pdfs,
            ["conflict-image-current", "conflict-image-incoming"],
            false,
        ))
        .when(show_external, |column| {
            column.child(
                div()
                    .id("conflict-raw-shared-actions")
                    .role(gpui::Role::Group)
                    .aria_label(Msg::ConflictOpenBothExternal.t())
                    .flex()
                    .flex_shrink_0()
                    .px(theme::scaled_px(12.))
                    .py(theme::scaled_px(6.))
                    .child(super::e2e::measure_control(
                        "conflict-open-both-external",
                        external_compare_button(path, cx),
                    )),
            )
        })
        .into_any_element()
}

fn raw_action_row(
    mode: &super::conflict_view::ConflictMode,
    path: &Path,
    side: SelectionSide,
    writer_busy: bool,
    cx: &mut Context<ConflictView>,
) -> Option<gpui::AnyElement> {
    let source = mode.buffer.side_raw_meta(path, side);
    let target = if source.is_some() {
        RawSideTarget::Adopt(side)
    } else {
        RawSideTarget::Deleted
    };
    let key = match side {
        SelectionSide::Current => "current",
        SelectionSide::Incoming => "incoming",
    };
    let resolution = mode.buffer.raw_resolution(path);
    let used = resolution.is_some_and(|resolution| {
        source.as_ref().is_some_and(|(oid, mode)| {
            resolution.oid.to_string() == *oid && resolution.mode == *mode
        })
    });
    let decided = resolution.is_some();
    let mut row = div().flex().items_center().gap(theme::scaled_px(8.));
    if decided {
        let message = if used {
            Msg::ConflictUsed
        } else {
            Msg::ConflictNotUsed
        };
        let id = format!("conflict-raw-{key}-state");
        #[cfg(feature = "gui-e2e")]
        super::conflict_editor_a11y::record_status(&id, message.t());
        row = row.child(
            div()
                .id(SharedString::from(id.clone()))
                .role(gpui::Role::Status)
                .aria_label(message.t())
                .text_size(theme::scaled_px(11.5))
                .text_color(rgb(theme().text_main))
                .border_1()
                .border_color(rgb(theme().text_main))
                .rounded(theme::scaled_px(4.))
                .px(theme::scaled_px(5.))
                .child(message.t())
                .relative()
                .child(super::e2e::measure_inside(id)),
        );
    }
    let id = format!(
        "conflict-raw-{key}-{}",
        if decided { "change" } else { "use" }
    );
    // Caption/probe changes must not replace the focused push button's identity.
    let button_id = format!("conflict-raw-{key}-action");
    let label = if decided {
        Msg::ConflictChange
    } else {
        match target {
            RawSideTarget::Adopt(_) => Msg::ConflictUseThisSide,
            RawSideTarget::Deleted => Msg::ConflictUseDeletedSide,
        }
    };
    let name = if decided {
        Msg::ConflictChangeAccessible
    } else {
        match target {
            RawSideTarget::Adopt(_) => Msg::ConflictUseThisSideAccessible,
            RawSideTarget::Deleted => Msg::ConflictUseDeletedSide,
        }
    };
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record(&id, name.t(), label.t(), "");
    let path = path.to_path_buf();
    let revision = mode.revision.clone();
    let button = super::conflict_editor::tool_button(
        &button_id,
        label.t(),
        super::button_style::ButtonRole::NeutralTinted,
        cx.listener(move |view, _, window, cx| {
            if view.writer_busy
                || !view.mode.as_ref().is_some_and(|mode| {
                    mode.revision == revision
                        && mode
                            .selected_file
                            .and_then(|i| mode.session.files.get(i))
                            .is_some_and(|file| file.path == path)
                })
            {
                return;
            }
            if decided {
                view.conflict_change_raw_choice(&path);
            } else {
                match target {
                    RawSideTarget::Adopt(side) => view.conflict_apply_choice(
                        &path,
                        match side {
                            SelectionSide::Current => kagi_git::ResolutionChoice::Current,
                            SelectionSide::Incoming => kagi_git::ResolutionChoice::Incoming,
                        },
                        cx,
                    ),
                    RawSideTarget::Deleted => {
                        let intent = view.freeze_delete_intent(&path);
                        let app = view.app.clone();
                        cx.spawn_in(window, async move |_, cx| {
                            let _ = app
                                .update_in(cx, |app, _, cx| app.accept_conflict_intent(intent, cx));
                        })
                        .detach();
                    }
                }
            }
            cx.notify();
        }),
        cx,
    )
    .aria_label(name.t())
    .disabled(writer_busy)
    .aria_description(if writer_busy {
        Msg::OpInProgress.t()
    } else {
        ""
    });
    Some(
        row.child(super::e2e::measure_control(id, button))
            .into_any_element(),
    )
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
