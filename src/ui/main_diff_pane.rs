//! ADR-0121 B2: the full-width main diff (T-UI-003) as a fat entity
//! (ADR-0117 template), registered as the `CenterPane::Diff` [`super::workspace::WorkspaceItem`].
//!
//! What moved in here from `KagiApp`:
//! - the [`MainDiffView`] itself (was `KagiApp.main_diff: Option<MainDiffView>`;
//!   the field is now `Option<Entity<MainDiffPane>>`),
//! - the diff-list scroll state (was `KagiApp.main_diff_scroll_handle` — the
//!   `ListState` now lives and dies with the pane),
//! - the off-thread highlight swap-in (was `KagiApp.pending_diff_highlight` +
//!   `apply_pending_highlights`): the spawn's `this.update` now targets the
//!   pane entity, so a result that arrives after the diff was closed is
//!   dropped by the dead-weak-handle guard instead of a render-time check.
//!
//! The File History / Editor Workspace *embedded* diff panes are untouched:
//! they keep their own `MainDiffView` + `ListState` fields and render via
//! `render_helpers::render_diff_list` directly, exactly as before.

use gpui::{prelude::*, Context, Entity, ListState, WeakEntity, Window};
use gpui_component::button::Button;
use gpui_component::Sizable as _;

use super::diff_view::{build_main_diff_view, MainDiffSource, MainDiffView, RowHighlights};
use super::render_helpers::{new_diff_list_state, render_diff_list};
use super::KagiApp;

/// Fat entity for the standalone (center-slot) main diff.
pub struct MainDiffPane {
    /// The diff currently shown. Replaced in place on j/k file steps and on
    /// re-opens while the pane is up, so the `ListState` keeps the same
    /// lifecycle the old persistent `KagiApp.main_diff_scroll_handle` had
    /// (reset-to-top only when the row count changes — see
    /// `render_helpers::render_diff_list`).
    pub view: MainDiffView,
    /// T-UI-003 / T-DIFF-WRAP-001: `ListState` (variable-height) for the
    /// "main-diff-list" — see `render_helpers::render_diff_list` for the
    /// item-count sync/reset lifecycle.
    scroll: ListState,
    /// ADR-0117: parent handle for the header buttons (Back / History). Only
    /// upgraded from event listeners — never from the render path (re-entrancy).
    app: WeakEntity<KagiApp>,
}

impl MainDiffPane {
    pub fn new(view: MainDiffView, app: WeakEntity<KagiApp>) -> Self {
        Self {
            view,
            scroll: new_diff_list_state(),
            app,
        }
    }

    /// ADR-0109: apply an off-thread highlight result if the pane still shows
    /// the same commit file it was requested for (stale results — the user
    /// stepped to another file first — are discarded, as before).
    pub(crate) fn apply_highlights(&mut self, row: usize, file: usize, highlights: RowHighlights) {
        match self.view.source {
            MainDiffSource::Commit {
                row_index,
                file_index,
            } if row_index == row && file_index == file => {}
            _ => return,
        }
        // The only writer of `rows`, and the pane owns the sole strong handle
        // at this point, so `make_mut` is a no-copy in-place edit.
        let rows = std::sync::Arc::make_mut(&mut self.view.rows);
        for (row_i, row_highlights) in highlights {
            if let Some(super::diff_view::DiffRow::Line { highlights: hl, .. }) =
                rows.get_mut(row_i)
            {
                *hl = row_highlights;
            }
        }
    }
}

impl Render for MainDiffPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Standalone header buttons (moved from `render_main_diff_view`'s
        // `standalone: true` arm). "← Back" closes the diff; "History" opens
        // File History for the shown file (导线 #3).
        let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.app
                .update(cx, |app, cx| {
                    app.close_main_diff();
                    cx.notify();
                })
                .ok();
        });
        let history_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            // Read the source HERE, off `this` — this listener runs while the
            // pane entity is leased, so the app must not read it back out of
            // `self.main_diff` (that panicked: "cannot read MainDiffPane while
            // it is already being updated").
            let source = this.view.source.clone();
            this.app
                .update(cx, |app, cx| {
                    app.open_file_history_from_main_diff(source, cx);
                    cx.notify();
                })
                .ok();
        });
        let leading = Button::new("main-diff-back")
            .label("\u{2190} Back")
            // `outline`, not `ghost`: a ghost button paints no background of
            // its own, so it took the header bar's colour exactly and read as
            // plain text rather than a control (user report). Outline gives it
            // the theme's input background plus a border.
            .outline()
            .small()
            .on_click(back_click)
            .into_any_element();
        let ext_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            // Same lease rule as history_click: read the source off `this`.
            let source = this.view.source.clone();
            this.app
                .update(cx, |app, cx| {
                    if let Some((path, _)) = app.main_diff_source_ref(&source, cx) {
                        app.open_in_external_editor(&path, None, cx);
                    }
                    cx.notify();
                })
                .ok();
        });
        let trailing = gpui::div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(
                Button::new("main-diff-ext-editor")
                    .label(gpui::SharedString::from(
                        crate::ui::i18n::Msg::OpenInExternalEditor.t(),
                    ))
                    .outline()
                    .small()
                    .on_click(ext_click),
            )
            .child(
                Button::new("main-diff-history")
                    .label("History")
                    .outline()
                    .small()
                    .on_click(history_click),
            )
            .into_any_element();

        render_diff_list::<MainDiffPane>(
            self.view.clone(),
            Some(leading),
            Some(trailing),
            self.scroll.clone(),
            cx,
        )
    }
}

impl KagiApp {
    /// Show `view` in the main diff pane: update the live pane in place (j/k
    /// steps, re-opens) or create the entity on first open. Returns the pane
    /// so callers can chain a highlight spawn onto it.
    pub(crate) fn show_main_diff(
        &mut self,
        view: MainDiffView,
        cx: &mut Context<Self>,
    ) -> Entity<MainDiffPane> {
        match self.main_diff.clone() {
            Some(pane) => {
                pane.update(cx, |p, cx| {
                    p.view = view;
                    cx.notify();
                });
                pane
            }
            None => {
                let weak = cx.weak_entity();
                let pane = cx.new(|_| MainDiffPane::new(view, weak));
                self.main_diff = Some(pane.clone());
                pane
            }
        }
    }
}

/// Everything a reload needs to put the open diff back on screen. Captured
/// *before* the new snapshot is installed, because the sweep in
/// `apply_reload_data` (`invalidate_caches_for_row_renumber`) drops the pane
/// and renumbers the rows the source points at.
pub(crate) struct MainDiffRestore {
    pane: Entity<MainDiffPane>,
    source: MainDiffSource,
    /// The file on screen, resolved while the old row indices still held.
    path: Option<std::path::PathBuf>,
    /// `Commit` source: the commit its row pointed at.
    commit: Option<kagi_git::CommitId>,
    /// `Staged` / `Unstaged` source: the repository the Commit Panel was
    /// staging into. #473 — a linked worktree's panel must not be re-read from
    /// the tab's repository.
    wip_repo: Option<std::path::PathBuf>,
}

impl KagiApp {
    /// Take hold of the open diff before a reload sweeps it away. `None` when
    /// nothing is open.
    pub(crate) fn capture_main_diff(&self, cx: &Context<Self>) -> Option<MainDiffRestore> {
        let pane = self.main_diff.clone()?;
        let source = pane.read(cx).view.source.clone();
        let path = self.main_diff_source_ref(&source, cx).map(|(p, _)| p);
        let commit = match source {
            MainDiffSource::Commit { row_index, .. } => self.commit_id_for_row(row_index),
            _ => None,
        };
        let wip_repo = match source {
            MainDiffSource::Staged { .. } | MainDiffSource::Unstaged { .. } => {
                self.commit_panel_repo_path(cx)
            }
            _ => None,
        };
        Some(MainDiffRestore {
            pane,
            source,
            path,
            commit,
            wip_repo,
        })
    }

    /// Put the captured diff back, refreshed against the snapshot the reload
    /// installed. Closing it on every reload is what threw a reader back to the
    /// graph each time an auto-fetch or the FS watcher fired.
    ///
    /// The pane **entity** is reused rather than rebuilt, so its `ListState` —
    /// the scroll position inside the diff — survives with it. What each source
    /// needs re-reading differs, and a source that can no longer be resolved
    /// (the commit left the graph, the file is no longer changed, the read
    /// errors) stays closed rather than freezing content the repository no
    /// longer has.
    pub(crate) fn restore_main_diff(&mut self, prev: MainDiffRestore, cx: &mut Context<Self>) {
        let MainDiffRestore {
            pane,
            source,
            path,
            commit,
            wip_repo,
        } = prev;
        match source {
            // A commit's diff is immutable: nothing to re-read, just re-point
            // the row the graph renumbered so j/k stepping and the History
            // button still resolve.
            MainDiffSource::Commit { file_index, .. } => {
                let Some(commit) = commit else { return };
                let Some(&row_index) = self.view().commit_row_index.get(&commit) else {
                    return;
                };
                pane.update(cx, |p, _| {
                    p.view.source = MainDiffSource::Commit {
                        row_index,
                        file_index,
                    }
                });
                self.main_diff = Some(pane);
            }
            // The compare list was re-read first (`restore_compare`); find the
            // same file in it again — its index moves as files enter and leave
            // the comparison — and re-read the diff into the pane.
            MainDiffSource::Compare { .. } => {
                let Some(path) = path else { return };
                let Some(file_index) = self
                    .compare_view
                    .as_ref()
                    .and_then(|p| p.read(cx).view.files.iter().position(|f| f.path == path))
                else {
                    return;
                };
                self.main_diff = Some(pane);
                self.open_main_diff_compare(file_index, cx);
            }
            MainDiffSource::Staged { path } => {
                self.restore_wip_diff(pane, path, true, wip_repo, cx)
            }
            MainDiffSource::Unstaged { path } => {
                self.restore_wip_diff(pane, path, false, wip_repo, cx)
            }
            // ADR-0145: the PR conflict preview is computed, with no file
            // behind it to re-read.
            MainDiffSource::Synthetic => {}
        }
    }

    /// Re-read a Commit Panel file's diff into `pane`. Leaves the pane closed
    /// when the file has nothing left to show — committed, discarded or staged
    /// away while the reload was in flight — so a commit still returns the user
    /// to the graph instead of freezing the diff it just consumed.
    fn restore_wip_diff(
        &mut self,
        pane: Entity<MainDiffPane>,
        path: std::path::PathBuf,
        staged: bool,
        wip_repo: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        // #473: read from the repository the panel was staging into, which for
        // a linked worktree's panel is not the tab's.
        let foreign = wip_repo
            .filter(|p| Some(p) != self.repo_path.as_ref())
            .and_then(|p| kagi_git::Backend::open(&p).ok());
        let result = {
            let repo = match (&foreign, self.repo_session.as_ref()) {
                (Some(backend), _) => backend,
                (None, Some(session)) => session.backend(),
                (None, None) => return,
            };
            if staged {
                repo.staged_file_diff(&path)
            } else {
                repo.unstaged_file_diff(&path)
            }
        };
        let file_diff = match result {
            Ok(fd) => fd,
            Err(e) => {
                klog!("commit-panel diff refresh error: {}", e);
                return;
            }
        };
        if file_diff.hunks.is_empty() && !file_diff.is_binary {
            return;
        }
        let source = if staged {
            MainDiffSource::Staged { path: path.clone() }
        } else {
            MainDiffSource::Unstaged { path: path.clone() }
        };
        let mut view = build_main_diff_view(&file_diff, &path, 0, source.clone());
        view.images = self.diff_images_for(&file_diff, &source, &path);
        self.main_diff = Some(pane);
        self.show_main_diff(view, cx);
    }
}
