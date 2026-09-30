//! ADR-0121 B2: the full-width main diff (T-UI-003) as a fat entity
//! (ADR-0117 template), registered as the `CenterPane::Diff` [`super::workspace::WorkspaceItem`].
//!
//! What moved in here from `KagiApp`:
//! - the [`MainDiffView`] itself (was `KagiApp.main_diff: Option<MainDiffView>`;
//!   the field is now `Option<Entity<MainDiffPane>>`),
//! - the diff-list scroll state (was `KagiApp.main_diff_scroll_handle` — the
//!   `ListState` now lives and dies with the pane),
//! - the off-thread highlight swap-in: the pane is a
//!   [`DiffHighlightHost`], so a result that arrives after the diff was
//!   closed hits a dead weak handle, and one for rows the pane no longer
//!   shows is dropped by `diff_view::highlight` (#495).
//! - the off-thread Compare / Commit Panel diff read ([`MainDiffRead`]),
//!   superseded by `TabUiState::main_diff_req` (#495).
//!
//! The File History / Editor Workspace *embedded* diff panes are untouched:
//! they keep their own `MainDiffView` + `ListState` fields and render via
//! `render_helpers::render_diff_list` directly, exactly as before.

use super::tab_ui_state_ops::PaneRevalidation;
use gpui::SharedString;
use gpui::{prelude::*, Context, Entity, ListState, WeakEntity, Window};

use super::diff_view::highlight::DiffHighlightHost;
use super::diff_view::{
    build_main_diff_view, diff_line_counts, CompareTarget, CompareView, MainDiffSource,
    MainDiffView,
};
use super::render_helpers::{
    header_button, new_diff_list_state, render_diff_list, DiffHeader, HeaderFit,
};
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
    /// Parent handle for deferred header actions.
    app: WeakEntity<KagiApp>,
    /// Session that owns this retained pane.
    owner: crate::app::SessionId,
    /// #809: the header's buttons fall back to icons when it is too narrow.
    /// Public for the layout scenario, which reads the measured bounds.
    pub fit: HeaderFit,
}

impl MainDiffPane {
    pub fn new(view: MainDiffView, app: WeakEntity<KagiApp>, owner: crate::app::SessionId) -> Self {
        Self {
            view,
            scroll: new_diff_list_state(),
            app,
            owner,
            fit: HeaderFit::default(),
        }
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
            )
            .on_click(back_click),
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
            .child(
                self.fit.control(
                    "main-diff-ext-editor",
                    header_button(
                        "main-diff-ext-editor",
                        ext_label.clone(),
                        "icons/external-link.svg",
                        compact,
                    )
                    .on_click(ext_click),
                ),
            )
            .child(
                self.fit.control(
                    "main-diff-history",
                    header_button(
                        "main-diff-history",
                        history_label.clone(),
                        "icons/history.svg",
                        compact,
                    )
                    .on_click(history_click),
                ),
            )
            .into_any_element();

        render_diff_list::<MainDiffPane>(
            self.view.clone(),
            DiffHeader {
                fit: Some(self.fit.clone()),
                leading: Some(leading),
                trailing: Some(trailing),
                labels: vec![back_label, ext_label, history_label],
                ..DiffHeader::default()
            },
            self.scroll.clone(),
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
        self.bump_main_diff_req();
        match self.ui().main_diff.clone() {
            Some(pane) => {
                pane.update(cx, |p, cx| {
                    p.view.adopt(view);
                    cx.notify();
                });
                pane
            }
            None => {
                let weak = cx.weak_entity();
                let owner = self
                    .active_session()
                    .expect("main diff requires an attached session");
                let pane = cx.new(|_| MainDiffPane::new(view, weak, owner));
                if let Some(ui) = self.ui_mut() {
                    ui.main_diff = Some(pane.clone());
                }
                pane
            }
        }
    }

    /// Start a new main-diff request generation and return it.
    fn bump_main_diff_req(&mut self) -> u64 {
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
            refresh: true,
        };
        self.read_main_diff(repo, path, read, cx);
    }

    /// Read `path`'s diff for the main pane off the UI thread (#495) and show
    /// its text when it lands — unless the owner is no longer the tab on
    /// screen, or another install, read or close moved `main_diff_req` since.
    pub(crate) fn read_main_diff(
        &mut self,
        repo: std::path::PathBuf,
        path: std::path::PathBuf,
        read: MainDiffRead,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.active_session() else {
            return;
        };
        let req = self.bump_main_diff_req();
        if matches!(read, MainDiffRead::Commit { .. }) {
            self.with_ui(|ui| ui.main_diff_commit_read = Some(req));
        }
        let bg_path = path.clone();
        let bg_read = read.clone();
        // #355: a large diff explains itself once slow; no Skip.
        let slow = self.begin_slow_read(owner, Some(kagi_ui_core::slow_read::SlowRead::Diff), cx);
        #[cfg(feature = "gui-e2e")]
        let hold = MAIN_DIFF_READ_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let _slow = slow;
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            bg_read.run(&repo, &bg_path)
        });
        cx.spawn(async move |this, acx| {
            let landed = task.await;
            let _ = this.update(acx, |app, cx| {
                if app.active_session() == Some(owner) && app.ui().main_diff_req == req {
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
        let (file_diff, mut view) = match landed {
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
        } = &read
        {
            let key = (*row, *file_index);
            return self.land_commit_diff(file_diff, view, path, commit, key, *epoch, cx);
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
        view.images = self.diff_images_for(&file_diff, &view.source, path);
        self.show_main_diff(view, cx);
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
        file_diff: kagi_git::FileDiff,
        mut view: MainDiffView,
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
        let file_diff = std::sync::Arc::new(file_diff);
        if current {
            self.with_ui(|ui| {
                ui.diff_caches
                    .file_content
                    .insert((row, file_index), file_diff.clone())
            });
        }
        view.images = self.diff_images_for(&file_diff, &view.source, path);
        self.show_main_diff(view, cx);
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
    },
    /// A Commit Panel file. `refresh`: re-reading an open diff after a
    /// reload, which closes the pane when nothing is left to show.
    Wip { staged: bool, refresh: bool },
    /// A file of the selected commit on a content-cache miss (#829). `row`
    /// and `epoch` key the cache it fills and go stale on a renumber.
    Commit {
        commit: kagi_git::CommitId,
        row: usize,
        file_index: usize,
        epoch: u64,
    },
}

/// What a [`MainDiffRead`] produced: the raw diff (image blobs are read from
/// it on the UI thread) and its text-only view.
pub(crate) enum Landed {
    Show(Box<(kagi_git::FileDiff, MainDiffView)>),
    /// Nothing to show: no HEAD to compare against, or (refresh) no change left.
    Nothing,
    OpenFailed(String),
    Failed(String),
}

impl MainDiffRead {
    /// Open `repo` read-only, diff `path`, and build the text rows — all of
    /// the I/O and projection, none of it on the UI thread.
    fn run(&self, repo: &std::path::Path, path: &std::path::Path) -> Landed {
        let backend = match kagi_git::Backend::open(repo) {
            Ok(backend) => backend,
            Err(e) => return Landed::OpenFailed(e.to_string()),
        };
        let (result, source, file_index) = match self {
            MainDiffRead::Compare {
                base,
                target,
                file_index,
            } => {
                let result = match target {
                    CompareTarget::Head => match backend.head_commit_id() {
                        Some(head) => backend.compare_file_diff(base, &head, path),
                        None => return Landed::Nothing,
                    },
                    CompareTarget::WorkingTree => {
                        backend.compare_commit_to_workdir_file_diff(base, path)
                    }
                    CompareTarget::Commit(id) => backend.compare_file_diff(base, id, path),
                };
                let source = MainDiffSource::Compare {
                    base: base.clone(),
                    target: target.clone(),
                    file_index: *file_index,
                };
                (result, source, *file_index)
            }
            MainDiffRead::Wip { staged, refresh } => {
                let result = if *staged {
                    backend.staged_file_diff(path)
                } else {
                    backend.unstaged_file_diff(path)
                };
                if *refresh && matches!(&result, Ok(fd) if fd.hunks.is_empty() && !fd.is_binary) {
                    return Landed::Nothing;
                }
                let path = path.to_path_buf();
                let source = if *staged {
                    MainDiffSource::Staged { path }
                } else {
                    MainDiffSource::Unstaged { path }
                };
                (result, source, 0)
            }
            MainDiffRead::Commit {
                commit,
                row,
                file_index,
                ..
            } => {
                let source = MainDiffSource::Commit {
                    row_index: *row,
                    file_index: *file_index,
                    commit: Some(commit.clone()),
                };
                (backend.commit_file_diff(commit, path), source, *file_index)
            }
        };
        match result {
            Ok(file_diff) => {
                let view = build_main_diff_view(&file_diff, path, file_index, source);
                Landed::Show(Box::new((file_diff, view)))
            }
            Err(e) => Landed::Failed(e.to_string()),
        }
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
        self.with_ui(|ui| {
            ui.main_diff = None;
            ui.main_diff_req = ui.main_diff_req.wrapping_add(1);
        });
        // ADR-0121 B2: also drop a not-yet-promoted headless staging view.
        self.pending_headless_diff = None;
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
        if self.ui().conflict.is_none() {
            // No retained conflict pane: nothing to re-check.
            self.with_ui(|ui| ui.pane_revalidation = PaneRevalidation::Settled);
        } else if self.view().operation.is_none() {
            // Resolved or aborted while the tab was away — the pane goes.
            self.with_ui(|ui| {
                ui.conflict = None;
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
