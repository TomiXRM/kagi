//! ADR-0121 B2: the full-width main diff (T-UI-003) as a fat entity
//! (ADR-0117 template), registered as the `CenterPane::Diff` [`super::workspace::WorkspaceItem`].
//!
//! What moved in here from `KagiApp`:
//! - the [`MainDiffView`] itself (was `KagiApp.main_diff: Option<MainDiffView>`;
//!   the field is now `Option<Entity<MainDiffPane>>`),
//! - the diff-list scroll state (was `KagiApp.main_diff_scroll_handle` — the
//!   `VariableListState` now lives and dies with the pane),
//! - the off-thread highlight swap-in: the pane is a
//!   [`DiffHighlightHost`], so a result that arrives after the diff was
//!   closed hits a dead weak handle, and one for rows the pane no longer
//!   shows is dropped by `diff_view::highlight` (#495).
//! - the off-thread Compare / Commit Panel diff read ([`MainDiffRead`]),
//!   superseded by `TabUiState::main_diff_req` (#495).
//!
//! The File History / Editor Workspace *embedded* diff panes are untouched:
//! they keep their own `MainDiffView` + `VariableListState` fields and render via
//! `render_helpers::render_diff_list` directly, exactly as before.

mod ownership;
pub(crate) mod preparation;

use super::tab_ui_state_ops::PaneRevalidation;
use gpui::SharedString;
use gpui::{prelude::*, Context, Entity, WeakEntity, Window};
use kagi_ui_core::variable_list::VariableListState;

use super::diff_view::highlight::DiffHighlightHost;
use super::diff_view::pdf::{same_immutable_document, DiffPdfViewers, PreparedPdfPair};
use super::diff_view::{
    build_main_diff_view, diff_line_counts, CompareTarget, CompareView, DiffImagePair,
    MainDiffSource, MainDiffView,
};
use super::render_helpers::{
    header_button, new_diff_list_state, render_diff_list, DiffHeader, HeaderFit,
};
use super::KagiApp;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DRAFT_HANDOFF_HOLD: std::cell::RefCell<Option<(
        gpui::Task<()>, std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>> = const { std::cell::RefCell::new(None) };
}

/// Fat entity for the standalone (center-slot) main diff.
pub struct MainDiffPane {
    /// Source/projection changes reset; highlighting and ordinary paints do not.
    pub view: MainDiffView,
    /// Kagi-owned variable-height measurements for this diff only.
    scroll: VariableListState,
    list_layout: kagi_ui_core::diff_list::DiffListLayout,
    /// Mounted only after the worker result passes the session/visit/request guards.
    pdfs: Option<DiffPdfViewers>,
    /// Parent handle for deferred header actions.
    app: WeakEntity<KagiApp>,
    /// Session that owns this retained pane.
    owner: crate::app::SessionId,
    /// #809: the header's buttons fall back to icons when it is too narrow.
    /// Public for the layout scenario, which reads the measured bounds.
    pub fit: HeaderFit,
}

impl MainDiffPane {
    pub fn new(
        view: MainDiffView,
        app: WeakEntity<KagiApp>,
        owner: crate::app::SessionId,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|pane, cx| pane.retire_images(cx)).detach();
        Self {
            view,
            scroll: new_diff_list_state(),
            list_layout: Default::default(),
            pdfs: None,
            app,
            owner,
            fit: HeaderFit::default(),
        }
    }

    fn set_view(
        &mut self,
        view: MainDiffView,
        pdfs: Option<PreparedPdfPair>,
        cx: &mut Context<Self>,
    ) {
        let retain = self
            .pdfs
            .as_ref()
            .zip(pdfs.as_ref())
            .is_some_and(|(viewers, sources)| viewers.can_retain(sources, cx))
            && same_immutable_document(&self.view.source, &view.source);
        if !retain {
            if let Some(previous) = &self.pdfs {
                previous.retire(cx);
            }
            self.pdfs = pdfs.map(|sources| DiffPdfViewers::new(sources, cx));
        }
        self.retire_images(cx);
        self.view.adopt(view);
    }

    fn retire_images(&mut self, cx: &mut gpui::App) {
        if let Some(pair) = self.view.side_pair.take() {
            super::diff_view::retire_side_images([&pair.left, &pair.right], cx);
        }
    }

    pub(crate) fn focused_pdf_body(
        &self,
        window: &Window,
        cx: &gpui::App,
    ) -> Option<gpui::FocusHandle> {
        self.pdfs.as_ref()?.focused_body(window, cx)
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_viewer_for_e2e(
        &self,
        old: bool,
        cx: &gpui::App,
    ) -> Option<Entity<kagi_ui_editor::PdfViewer>> {
        self.pdfs.as_ref()?.viewer(old, cx)
    }

    #[cfg(feature = "gui-e2e")]
    pub fn pdf_scroll_synchronized_for_e2e(&self, cx: &gpui::App) -> bool {
        self.pdfs.as_ref().is_some_and(|pdfs| pdfs.synchronized(cx))
    }

    /// The exact selection token used by the production list renderer.
    #[cfg(feature = "gui-e2e")]
    pub fn selection_token_for_e2e(&self) -> u64 {
        self.view.height_source.token()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn diff_row_bounds_for_e2e(
        &self,
        index: usize,
    ) -> (
        gpui::Bounds<gpui::Pixels>,
        Option<gpui::Bounds<gpui::Pixels>>,
    ) {
        (
            self.scroll.viewport_bounds(),
            self.scroll.bounds_for_item(index),
        )
    }

    #[cfg(feature = "gui-e2e")]
    pub fn reveal_diff_row_for_e2e(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = if super::theme::diff_split() {
            let projection = super::diff_split::split_projection(&self.view.rows);
            super::diff_split::split_index_of(&projection.rows, index).unwrap_or(index)
        } else {
            index
        };
        self.scroll.scroll_to_reveal_item(item);
        cx.notify();
    }
}

impl Drop for MainDiffPane {
    fn drop(&mut self) {
        super::diff_selection::retire_surface(self.list_layout.selection_owner());
    }
}

impl DiffHighlightHost for MainDiffPane {
    const LOG_READY: bool = true;
    fn for_each_diff(&mut self, visit: &mut dyn FnMut(&mut MainDiffView)) {
        visit(&mut self.view);
    }
}

impl Render for MainDiffPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Standalone header buttons (moved from `render_main_diff_view`'s
        // `standalone: true` arm). "← Back" closes the diff; "History" opens
        // File History for the shown file (导线 #3).
        let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            let owner = this.owner;
            this.app
                .update(cx, move |app, cx| {
                    if app.active_session() == Some(owner) {
                        app.close_main_diff();
                        cx.notify();
                    }
                })
                .ok();
        });
        let history_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            // Read the source HERE, off `this` — this listener runs while the
            // pane entity is leased, so the app must not read it back out of
            // the owner's `main_diff` slot (that panicked: "cannot read
            // MainDiffPane while it is already being updated").
            let source = this.view.source.clone();
            let owner = this.owner;
            this.app
                .update(cx, move |app, cx| {
                    if app.pane_mutation_admitted(owner) {
                        app.open_file_history_from_main_diff(source, cx);
                        cx.notify();
                    }
                })
                .ok();
        });
        let compact = self.fit.compact();
        let back_label = SharedString::from("\u{2190} Back");
        let ext_label = SharedString::from(crate::ui::i18n::Msg::OpenInExternalEditor.t());
        let history_label = SharedString::from("History");
        let leading = self.fit.control(
            "main-diff-back",
            header_button(
                "main-diff-back",
                back_label.clone(),
                "icons/arrow-left.svg",
                compact,
                |button| button.on_click(back_click),
            ),
        );
        let ext_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            // Same lease rule as history_click: read the source off `this`.
            let source = this.view.source.clone();
            let owner = this.owner;
            this.app
                .update(cx, move |app, cx| {
                    if app.pane_mutation_admitted(owner) {
                        if let Some((path, _)) = app.main_diff_source_ref(&source, cx) {
                            app.open_in_external_editor(&path, None, cx);
                        }
                        cx.notify();
                    }
                })
                .ok();
        });
        let trailing = gpui::div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(self.fit.control(
                "main-diff-ext-editor",
                header_button(
                    "main-diff-ext-editor",
                    ext_label.clone(),
                    "icons/external-link.svg",
                    compact,
                    |button| button.on_click(ext_click),
                ),
            ))
            .child(self.fit.control(
                "main-diff-history",
                header_button(
                    "main-diff-history",
                    history_label.clone(),
                    "icons/history.svg",
                    compact,
                    |button| button.on_click(history_click),
                ),
            ))
            .into_any_element();

        render_diff_list::<MainDiffPane>(
            self.view.clone(),
            DiffHeader {
                fit: Some(self.fit.clone()),
                leading: Some(leading),
                trailing: Some(trailing),
                pdfs: self.pdfs.clone(),
                labels: vec![back_label, ext_label, history_label],
                ..DiffHeader::default()
            },
            self.scroll.clone(),
            &self.list_layout,
            // #842: a Commit Panel side carries Stage / Unstage hunk.
            crate::ui::diff_view::hunk_action::HunkAction::for_source(
                &self.view.source,
                self.app.clone(),
                self.owner,
                self.view.hunk_approvals.clone(),
            ),
            super::diff_selection::SelectionOwner::Main(self.owner),
            cx,
        )
    }
}

impl KagiApp {
    /// Show `view` in the main diff pane: update the live pane in place (j/k
    /// steps, re-opens) or create the entity on first open. Returns the pane.
    /// An in-place update with unchanged text keeps the highlighted rows
    /// ([`MainDiffView::adopt`]). Supersedes any main-diff read still out.
    pub(crate) fn show_main_diff(
        &mut self,
        view: MainDiffView,
        cx: &mut Context<Self>,
    ) -> Entity<MainDiffPane> {
        self.show_prepared_main_diff(view, None, cx)
    }

    fn show_prepared_main_diff(
        &mut self,
        view: MainDiffView,
        pdfs: Option<PreparedPdfPair>,
        cx: &mut Context<Self>,
    ) -> Entity<MainDiffPane> {
        self.bump_main_diff_req();
        match self.ui().main_diff.clone() {
            Some(pane) => {
                pane.update(cx, |p, cx| {
                    p.set_view(view, pdfs, cx);
                    cx.notify();
                });
                pane
            }
            None => {
                let weak = cx.weak_entity();
                let owner = self
                    .active_session()
                    .expect("main diff requires an attached session");
                let pane = cx.new(|cx| {
                    let pdfs = pdfs.map(|sources| DiffPdfViewers::new(sources, cx));
                    let mut pane = MainDiffPane::new(view, weak, owner, cx);
                    pane.pdfs = pdfs;
                    pane
                });
                if let Some(ui) = self.ui_mut() {
                    ui.main_diff = Some(pane.clone());
                }
                pane
            }
        }
    }

    /// Start a new main-diff request generation and return it.
    pub(crate) fn bump_main_diff_req(&mut self) -> u64 {
        self.ui_mut().map_or(0, |ui| {
            ui.main_diff_req = ui.main_diff_req.wrapping_add(1);
            ui.main_diff_req
        })
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
        let pane = self.ui().main_diff.clone()?;
        let source = pane.read(cx).view.source.clone();
        let path = self.main_diff_source_ref(&source, cx).map(|(p, _)| p);
        let commit = match source {
            MainDiffSource::Commit { ref commit, .. } => commit.clone(),
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
    /// The pane **entity** is reused rather than rebuilt, so its retained geometry —
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
                        commit: Some(commit.clone()),
                    }
                });
                if let Some(ui) = self.ui_mut() {
                    ui.main_diff = Some(pane);
                }
            }
            // The compare list was re-read first (`restore_compare`); find the
            // same file in it again — its index moves as files enter and leave
            // the comparison — and re-read the diff into the pane.
            MainDiffSource::Compare { .. } => {
                let Some(path) = path else { return };
                let Some(file_index) = self
                    .ui()
                    .compare_view
                    .as_ref()
                    .and_then(|p| p.read(cx).view().files.iter().position(|f| f.path == path))
                else {
                    return;
                };
                if let Some(ui) = self.ui_mut() {
                    ui.main_diff = Some(pane);
                }
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

    /// Re-read a Commit Panel file's diff into `pane`, off the UI thread. The
    /// pane stays up with its current content meanwhile, so a reload does not
    /// bounce the reader to the graph; if the file then has nothing left to
    /// show — committed, discarded or staged away — or the read fails, the
    /// pane closes rather than freezing a diff the repository no longer has.
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
        let Some(repo) = wip_repo.or_else(|| self.repo_path.clone()) else {
            return;
        };
        if let Some(ui) = self.ui_mut() {
            ui.main_diff = Some(pane);
        }
        let read = MainDiffRead::Wip {
            staged,
            rename_from: None,
            refresh: true,
        };
        self.read_main_diff(repo, path, read, cx);
    }

    /// Read the complete text/image/PDF view off-thread. Only the frozen session,
    /// visit, request, published model and source owner may install its result.
    pub(crate) fn read_main_diff(
        &mut self,
        repo: std::path::PathBuf,
        path: std::path::PathBuf,
        read: MainDiffRead,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
        else {
            return;
        };
        let Some(source_owner) = read.source_owner(self, cx) else {
            return;
        };
        let req = self.bump_main_diff_req();
        let publish_gen = self.ui().view_publish_gen;
        if matches!(read, MainDiffRead::Commit { .. }) {
            self.with_ui(|ui| ui.main_diff_commit_read = Some(req));
        }
        let bg_path = path.clone();
        let bg_read = read.clone();
        let bg_repo = repo.clone();
        // #355: a large diff explains itself once slow; no Skip.
        let slow = self.begin_slow_read(
            owner.session,
            Some(kagi_ui_core::slow_read::SlowRead::Diff),
            cx,
        );
        #[cfg(feature = "gui-e2e")]
        let hold = MAIN_DIFF_READ_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let _slow = slow;
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            bg_read.run(&bg_repo, &bg_path)
        });
        cx.spawn(async move |this, acx| {
            let landed = task.await;
            let _ = this.update(acx, |app, cx| {
                let current = app
                    .active_session()
                    .and_then(|id| app.app_sessions.attachment(id));
                if current.is_some_and(|current| {
                    current.session == owner.session && current.visit == owner.visit
                }) && app.ui().main_diff_req == req
                    && (matches!(read, MainDiffRead::Commit { .. })
                        || app.ui().view_publish_gen == publish_gen)
                    && read.source_owner(app, cx) == Some(source_owner)
                    && read.is_current(app, &repo, &path, cx)
                {
                    app.land_main_diff(read, &path, landed, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Apply a current [`MainDiffRead`] result, with the contract lines the
    /// synchronous openers used to print.
    fn land_main_diff(
        &mut self,
        read: MainDiffRead,
        path: &std::path::Path,
        landed: Landed,
        cx: &mut Context<Self>,
    ) {
        let refresh = matches!(read, MainDiffRead::Wip { refresh: true, .. });
        let (file_diff, view, pdfs) = match landed {
            Landed::Show(shown) => *shown,
            Landed::Nothing if refresh => return self.close_main_diff(),
            Landed::Nothing => return,
            Landed::OpenFailed(e) | Landed::Failed(e) if refresh => {
                klog!("commit-panel diff refresh error: {}", e);
                return self.close_main_diff();
            }
            Landed::OpenFailed(e) if matches!(read, MainDiffRead::Wip { .. }) => {
                return klog!("commit-panel diff: repo open error: {}", e);
            }
            Landed::OpenFailed(e) | Landed::Failed(e) => {
                return match read {
                    MainDiffRead::Compare { .. } => klog!("compare diff error: {}", e),
                    MainDiffRead::Wip { .. } => klog!("commit-panel diff error: {}", e),
                    MainDiffRead::Commit { .. } => klog!("diff error: {}", e),
                };
            }
        };
        if let MainDiffRead::Commit {
            commit,
            row,
            file_index,
            epoch,
            ..
        } = &read
        {
            let key = (*row, *file_index);
            return self.land_commit_diff(file_diff, view, pdfs, path, commit, key, *epoch, cx);
        }
        if !refresh {
            let (added, removed) = diff_line_counts(&file_diff);
            if matches!(read, MainDiffRead::Wip { .. }) {
                klog!(
                    "commit-panel diff: {} (+{} -{})",
                    path.display(),
                    added,
                    removed
                )
            } else {
                klog!(
                    "diff: {} hunks={} (+{} -{})",
                    path.display(),
                    file_diff.hunks.len(),
                    added,
                    removed
                )
            }
            klog!(
                "main-diff: open {} rows={} highlight={}",
                path.display(),
                view.rows.len(),
                view.lang.unwrap_or("none")
            );
        }
        log_diff_images(&view, path);
        self.show_prepared_main_diff(view, pdfs, cx);
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static MAIN_DIFF_READ_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next off-thread main-diff read until `hold` completes, so a
    /// reload can land while it is still out (#829; Tier A only).
    pub fn hold_next_main_diff_read_for_e2e(hold: gpui::Task<()>) {
        MAIN_DIFF_READ_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
}

impl KagiApp {
    /// #829: a commit file's diff read off the UI thread, landing with the
    /// contract line the synchronous opener printed. It fills the per-(row,
    /// file) cache only while those rows are still the rows on screen; after a
    /// renumber (a reload or activation read published meanwhile) the diff is
    /// re-anchored to its commit's new row — as a diff opened just before the
    /// read would have been (#722) — or dropped when the commit is gone.
    #[allow(clippy::too_many_arguments)]
    fn land_commit_diff(
        &mut self,
        file_diff: std::sync::Arc<kagi_git::FileDiff>,
        mut view: MainDiffView,
        pdfs: Option<PreparedPdfPair>,
        path: &std::path::Path,
        commit: &kagi_git::CommitId,
        (row, file_index): (usize, usize),
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        let current = self.ui().cache_epoch == epoch;
        let row = if current {
            row
        } else {
            let Some(row) = self.row_for_commit_id(commit) else {
                return;
            };
            view.source = MainDiffSource::Commit {
                row_index: row,
                file_index,
                commit: Some(commit.clone()),
            };
            row
        };
        let (added, removed) = diff_line_counts(&file_diff);
        klog!(
            "diff: {} hunks={} (+{} -{})",
            path.display(),
            file_diff.hunks.len(),
            added,
            removed
        );
        if current {
            self.with_ui(|ui| {
                ui.diff_caches.file_content.insert(
                    (row, file_index),
                    super::diff_cache::CachedFileDiff {
                        diff: file_diff.clone(),
                        is_pdf: Some(pdfs.is_some()),
                    },
                )
            });
        }
        log_diff_images(&view, path);
        self.show_prepared_main_diff(view, pdfs, cx);
    }
}

/// #495: a main-pane diff read off the UI thread.
#[derive(Clone)]
pub(crate) enum MainDiffRead {
    /// A file of the open compare list.
    Compare {
        base: kagi_git::CommitId,
        target: CompareTarget,
        file_index: usize,
        rename_from: Option<std::path::PathBuf>,
    },
    /// A Commit Panel file. `refresh`: re-reading an open diff after a
    /// reload, which closes the pane when nothing is left to show.
    Wip {
        staged: bool,
        /// Frozen panel identity for initial opens; refresh resolves current status off-thread.
        rename_from: Option<std::path::PathBuf>,
        refresh: bool,
    },
    /// A file of the selected commit on a content-cache miss (#829). `row`
    /// and `epoch` key the cache it fills and go stale on a renumber.
    Commit {
        commit: kagi_git::CommitId,
        row: usize,
        file_index: usize,
        epoch: u64,
        /// Frozen Inspector rename identity; a path-filtered delta can report Added.
        rename_from: Option<std::path::PathBuf>,
        /// Reuse cached immutable text while gathering its document/image sides.
        cached: Option<std::sync::Arc<kagi_git::FileDiff>>,
    },
}

/// What a [`MainDiffRead`] produced: the raw diff and complete prepared view.
pub(crate) enum Landed {
    Show(
        Box<(
            std::sync::Arc<kagi_git::FileDiff>,
            MainDiffView,
            Option<PreparedPdfPair>,
        )>,
    ),
    /// Nothing to show: no HEAD to compare against, or (refresh) no change left.
    Nothing,
    OpenFailed(String),
    Failed(String),
}

/// Contract logging belongs to accepted foreground publication, not a worker
/// whose complete view may be superseded before it lands.
fn log_diff_images(view: &MainDiffView, path: &std::path::Path) {
    if let Some(pair) = &view.images {
        klog!(
            "diff-image: {} old={} new={}",
            path.display(),
            pair.old.is_some(),
            pair.new.is_some()
        );
    }
}

/// The active owner's open Compare + Main Diff panes, captured before a read
/// renumbers the commit rows so both can be re-anchored against the new rows
/// afterwards (ADR-0197 決定 3). Shared by the reload apply path
/// (`apply_reload_data`) and the tab-switch / activation read
/// (`load_repo_async`), which otherwise duplicated this capture/restore.
pub(crate) struct OpenPanes {
    compare: Option<CompareView>,
    main_diff: Option<MainDiffRestore>,
}

impl KagiApp {
    /// Capture the active owner's open panes before a renumbering read. Only
    /// meaningful when that read's owner is the tab on screen.
    pub(crate) fn capture_open_panes(&self, cx: &Context<Self>) -> OpenPanes {
        OpenPanes {
            compare: self
                .ui()
                .compare_view
                .as_ref()
                .map(|pane| pane.read(cx).view().clone()),
            main_diff: self.capture_main_diff(cx),
        }
    }

    /// Put the captured panes back, re-anchored against the rows the read
    /// installed. Compare first: the diff restore looks its file up in the
    /// refreshed compare list. A source that no longer resolves stays closed.
    pub(crate) fn restore_open_panes(&mut self, panes: OpenPanes, cx: &mut Context<Self>) {
        // #829: the newest request is a commit diff still being read. It is
        // immutable and lands re-anchored by commit id, so the sweep keeps it.
        // Of the diff it is about to replace, only a commit's is put back
        // meanwhile: re-pointing it issues no read that would supersede it.
        let carry = self.ui().main_diff_commit_read == Some(self.ui().main_diff_req);
        if let Some(ui) = self.ui_mut() {
            ui.main_diff = None;
            if !carry {
                // A read still out for the pane being swept must not reopen it.
                ui.main_diff_req = ui.main_diff_req.wrapping_add(1);
            }
        }
        if let Some(view) = panes.compare {
            self.restore_compare(view, cx);
        }
        let restore =
            |prev: &MainDiffRestore| !carry || matches!(prev.source, MainDiffSource::Commit { .. });
        if let Some(prev) = panes.main_diff.filter(restore) {
            self.restore_main_diff(prev, cx);
        }
    }

    /// T-UI-003: Close the main diff view and return to the commit graph.
    /// No-op when main_diff is None. A diff read still out is superseded, so
    /// it cannot reopen the pane (#495).
    pub fn close_main_diff(&mut self) {
        if let Some(owner) = self.active_session() {
            super::diff_selection::retire_owner(super::diff_selection::SelectionOwner::Main(owner));
        }
        self.with_ui(|ui| {
            ui.main_diff = None;
            ui.main_diff_req = ui.main_diff_req.wrapping_add(1);
        });
        // ADR-0121 B2: also drop a not-yet-promoted headless staging view.
        self.pending_headless_diff = None;
    }

    #[cfg(feature = "gui-e2e")]
    pub fn hold_next_draft_handoff_for_e2e(
        hold: gpui::Task<()>,
    ) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        DRAFT_HANDOFF_HOLD.with(|slot| {
            assert!(slot.borrow_mut().replace((hold, reached.clone())).is_none());
        });
        reached
    }

    #[cfg(feature = "gui-e2e")]
    pub fn clear_draft_handoff_hold_for_e2e() {
        DRAFT_HANDOFF_HOLD.with(|slot| drop(slot.borrow_mut().take()));
    }

    /// Returns true when the retained owner must not be removed by the caller.
    /// Operation disappearance is not proof that its last Result reached disk.
    pub(crate) fn clear_conflict_after_draft_handoff(
        &mut self,
        owner: crate::app::Attachment,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self
            .ui
            .get(&owner.session)
            .and_then(|ui| ui.conflict.clone())
        else {
            return false;
        };
        let save = pane.update(cx, |view, cx| {
            view.capture_result_for_handoff(cx);
            view.mode.as_ref().and_then(|mode| {
                view.has_unflushed_drafts()
                    .then(|| mode.buffer.queue_autosave())
            })
        });
        let Some(save) = save else {
            return false;
        };
        let Some(ui) = self.ui.get_mut(&owner.session) else {
            return true;
        };
        // The operation strip disappears with the operation. Keep its retained
        // Result reachable while preservation is pending or has failed.
        ui.conflict_hidden = false;
        ui.pane_revalidation = PaneRevalidation::AwaitingConflict;
        ui.conflict_detected = true;
        ui.conflict_detect_generation = ui.conflict_detect_generation.wrapping_add(1);
        let generation = ui.conflict_detect_generation;
        let read_key = self.reads.current_key(owner.session);
        if self.active_session() == Some(owner.session) {
            self.clear_conflict_continue_modal();
            self.clear_conflict_abort_modal();
        }
        let task = cx.background_spawn(async move {
            match save {
                Ok(ticket) => (ticket.flush(), Some(ticket)),
                Err(error) => (Err(error), None),
            }
        });
        #[cfg(feature = "gui-e2e")]
        let hold = DRAFT_HANDOFF_HOLD.with(|slot| slot.borrow_mut().take());
        cx.spawn(async move |this, acx| {
            let (result, ticket) = task.await;
            #[cfg(feature = "gui-e2e")]
            if let Some((hold, reached)) = hold {
                reached.store(true, std::sync::atomic::Ordering::Release);
                hold.await;
            }
            let _ = this.update(acx, |app, cx| {
                if !app.reads.is_fresh(read_key)
                    || app
                        .app_sessions
                        .attachment(owner.session)
                        .is_none_or(|current| {
                            current.visit != owner.visit
                                || current.path != owner.path
                                || current.worktree != owner.worktree
                        })
                    || app.ui.get(&owner.session).is_none_or(|ui| {
                        ui.conflict_detect_generation != generation
                            || ui.conflict.as_ref() != Some(&pane)
                    })
                    || app.reads.get(Some(owner.session)).operation.is_some()
                {
                    return;
                }
                if let Err(error) = result {
                    pane.update(cx, |view, _| {
                        view.note_draft_result(None, Err(error.to_string()))
                    });
                    let recording = kagi_git::backend::recording::finalize(
                        kagi_git::oplog::OpLogEntry::new(
                            "conflict-draft-preserve",
                            owner.path.display().to_string(),
                            kagi_git::ops::StateSummary {
                                head: "unobserved".into(),
                                dirty: "unobserved".into(),
                            },
                            kagi_git::oplog::OpOutcome::Failed {
                                error: error.to_string(),
                            },
                        )
                        .with_ref_moves(Some(Vec::new())),
                    );
                    app.present_operation_completion(
                        &recording,
                        app.active_session() == Some(owner.session),
                        false,
                        Some(super::i18n::Msg::conflict_draft_preserve_failed()),
                        None,
                        cx,
                    );
                    if let Some(ui) = app.ui.get_mut(&owner.session) {
                        ui.conflict_restore_failed = true;
                    }
                } else {
                    // A newer edit or queued snapshot cannot be acknowledged by
                    // this ticket. Keep its exact input/history for explicit retry.
                    let pending = pane.update(cx, |view, cx| {
                        view.capture_result_for_handoff(cx);
                        view.mode
                            .as_ref()
                            .is_some_and(|mode| mode.buffer.has_unflushed_drafts())
                    });
                    if pending || ticket.as_ref().is_none_or(|ticket| !ticket.is_current()) {
                        return;
                    }
                    if let Some(ui) = app.ui.get_mut(&owner.session) {
                        ui.conflict = None;
                        ui.conflict_restore_failed = false;
                        ui.pane_revalidation = PaneRevalidation::Settled;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// Revalidate **every** retained pane of the tab on screen against the
    /// read that was just published, then re-admit their mutations. `render`
    /// calls this every frame and it runs only for [`PaneRevalidation::Queued`],
    /// which the three publish seams set — so activation load, manual reload
    /// (Cmd+R), the watcher and a remote refresh all revalidate the same set,
    /// while re-showing a cached read on tab switch queues nothing. Adding a
    /// retained pane means adding it here (ADR-0197 決定 3 / #722).
    pub(crate) fn revalidate_retained_panes(&mut self, cx: &mut Context<Self>) {
        if self.ui().pane_revalidation != PaneRevalidation::Queued {
            return;
        }
        // Main Diff / Compare: re-anchor against the new rows. Capturing now
        // is equivalent to capturing before the read — accepting a read does
        // not touch the panes, so they still carry the pre-read source.
        let panes = self.capture_open_panes(cx);
        self.restore_open_panes(panes, cx);
        if !self.ui().conflict_merge_pending {
            self.refresh_commit_panel_after_reload(cx);
        }
        // Analyze + File History (HEAD-versioned, refreshed in place).
        self.refresh_overlays_after_reload(self.view().head_oid.clone(), cx);
        // Editor Workspace: the watcher does not follow a background tab, so
        // its tree / buffers / external-change banner are as stale as anything
        // else here and only this read says what the worktree now holds.
        self.revalidate_editor_workspace(cx);
        // Conflict is the one pane whose check is asynchronous: the read model
        // says whether an operation is in progress, but only a detector run
        // against *this* read can say whether it is still the same conflict.
        if self.ui().conflict.is_none() && self.view().operation.is_none() {
            // No retained pane or accepted operation: nothing to re-check.
            // A fresh conflicted read must re-arm detection under its own key,
            // even when an earlier speculative detection was superseded.
            self.with_ui(|ui| ui.pane_revalidation = PaneRevalidation::Settled);
        } else if self.view().operation.is_none() {
            if let Some(owner) = self.detect_owner() {
                if self.clear_conflict_after_draft_handoff(owner, cx) {
                    return;
                }
            }
            self.with_ui(|ui| {
                ui.conflict = None;
                ui.conflict_restore_failed = false;
                ui.pane_revalidation = PaneRevalidation::Settled;
            });
        } else {
            // Re-detect against the accepted read and stay refused until
            // `apply_conflict_detect` confirms the observation matches. It is
            // not enough that *an* operation is in progress: a conflict that
            // moved to another revision would otherwise re-admit Continue /
            // Skip against the old `ResolutionBuffer` (#722 P1).
            self.with_ui(|ui| {
                ui.conflict_detected = false;
                ui.pane_revalidation = PaneRevalidation::AwaitingConflict;
            });
            self.detect_conflict_mode_async(cx);
        }
    }
}
