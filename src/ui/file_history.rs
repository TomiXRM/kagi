//! Bin-side glue for the File History pane (ADR-0121 Phase C3).
//!
//! The view itself lives in `crates/kagi-ui-file-history` (Git-free). This
//! module keeps the app-owned side:
//!
//! - the history + diff loads (they need `kagi_git`), driven by the pane's
//!   [`FileHistoryEvent::HistoryLoadRequested`] / [`FileHistoryEvent::DiffLoadRequested`]
//!   requests and marshalled back via `seed_history` / the diff pane entity;
//! - [`FhDiffPane`], the diff-viewer slot the pane embeds as an `AnyView` —
//!   it reuses the bin's shared `MainDiffView` / `render_diff_list` pipeline,
//!   which is why the diff body stays out of the pane crate;
//! - the [`FileHistoryEvent`] subscription mapping close / jump-to-commit onto
//!   `KagiApp`.

pub use kagi_ui_file_history::*;

use super::*;

use kagi_git::FileHistoryEntryKind;

/// Host-side diff viewer slot for the File History pane (ADR-0121 C3).
///
/// Rendered inside the pane's list/diff split via the `diff_pane: AnyView`
/// slot. Holds the built [`MainDiffView`] for the selected entry (or `None` →
/// the "no diff" placeholder) plus the diff list scroll state and the
/// monotonic per-diff request token (`req`) that discards superseded async
/// diff loads (rapid arrowing — including A→B→A on a WIP entry whose
/// working-tree contents can change between reads).
pub struct FhDiffPane {
    /// Diff of the selected entry, reusing the existing diff renderer.
    pub diff: Option<MainDiffView>,
    /// T-DIFF-WRAP-001: `ListState` (variable-height) for the diff viewer
    /// list — see `render_helpers::render_diff_list` for the item-count
    /// sync/reset lifecycle.
    pub scroll: gpui::ListState,
    /// Monotonic per-diff request token, bumped on every diff load.
    pub req: u64,
}

impl Render for FhDiffPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.diff.clone() {
            // ADR-0117: render the diff list directly on this pane's context
            // (no standalone Back/History buttons — FH has its own Back).
            Some(view) => render_helpers::render_diff_list::<FhDiffPane>(
                view,
                None,
                None,
                self.scroll.clone(),
                cx,
            )
            .into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("No diff available for this entry."))
                .into_any_element(),
        }
    }
}

/// The pane's diff slot, recovered from its opaque `AnyView` handle.
fn fh_diff_pane(view: &Entity<FileHistoryView>, cx: &App) -> Option<Entity<FhDiffPane>> {
    view.read(cx)
        .diff_pane
        .clone()
        .downcast::<FhDiffPane>()
        .ok()
}

impl KagiApp {
    /// Open the File History view for `rel_path` (repo-relative). ADR-0117: this
    /// builds the `Entity<FileHistoryView>` (in Loading state) and kicks off its
    /// own async history load (read-only — no `busy_op` gate). ADR-0121 C3: the
    /// pane crate is Git-free, so the app owns the loads — the subscription
    /// below answers the pane's load requests and maps close / jump-to-commit
    /// onto `KagiApp`. Callers: the inspector / main-diff "History" entry
    /// points.
    pub fn open_file_history(
        &mut self,
        rel_path: PathBuf,
        origin: Option<CommitId>,
        cx: &mut Context<Self>,
    ) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        klog!("file-history: open {}", rel_path.display());

        let branch = SharedString::from(self.active_view.status_summary.branch.clone());
        let state = FileHistoryState::new(rel_path, branch);

        // The entity holds a shared clone of the geom cell (the divider-drag
        // reads it), `panel_width`, and the host diff slot.
        let geom = self.file_history_geom.clone();
        let panel_width = self.panel_width;
        let diff_pane = cx.new(|_| FhDiffPane {
            diff: None,
            scroll: render_helpers::new_diff_list_state(),
            req: 0,
        });
        let view = cx.new(|_| FileHistoryView::new(state, geom, panel_width, diff_pane.into()));

        // The pane's outward surface: it emits, the app decides (ADR-0121 C3).
        // `repo_path` is captured at open time — the FH session's repo is
        // constant for the entity's life (FH closes on repo/tab switch).
        cx.subscribe(&view, move |app, view, event, cx| match event {
            FileHistoryEvent::CloseRequested => {
                app.close_file_history();
                cx.notify();
            }
            FileHistoryEvent::JumpToCommit(id) => {
                app.close_file_history();
                app.jump_to_commit(id);
                cx.notify();
            }
            FileHistoryEvent::HistoryLoadRequested {
                generation,
                origin,
                emit_loaded,
            } => app.fh_load_history(
                &view,
                repo_path.clone(),
                *generation,
                origin.clone(),
                *emit_loaded,
                cx,
            ),
            FileHistoryEvent::DiffLoadRequested => app.fh_load_diff(&view, repo_path.clone(), cx),
        })
        .detach();

        // Kick off the initial load on the (now fully-constructed) entity.
        view.update(cx, |v, cx| v.request_load(origin, true, cx));
        self.file_history = Some(view);
        // Record the HEAD this history reflects so a later reload only reloads it
        // in place when HEAD actually moves (see `refresh_overlays_after_reload`).
        self.file_history_head = self.active_view.head_oid.clone();
    }

    /// Run the async history load the pane requested and marshal the result
    /// back into it via [`FileHistoryView::seed_history`] (which applies the
    /// `generation` staleness guard and the `[kagi]` loaded-line contract).
    fn fh_load_history(
        &mut self,
        view: &Entity<FileHistoryView>,
        repo_path: PathBuf,
        generation: u64,
        origin: Option<CommitId>,
        emit_loaded: bool,
        cx: &mut Context<Self>,
    ) {
        // A (re)load invalidates the current diff: clear the slot and bump its
        // request token so an in-flight older diff load can't land
        // (pre-extraction this was `data.diff = None` + the generation guard).
        if let Some(pane) = fh_diff_pane(view, cx) {
            pane.update(cx, |p, cx| {
                p.diff = None;
                p.req = p.req.wrapping_add(1);
                cx.notify();
            });
        }
        let (follow, req_path) = {
            let d = &view.read(cx).data;
            (d.follow_renames, d.rel_path.clone())
        };
        let task = cx.background_spawn(async move {
            let req = kagi_git::FileHistoryRequest {
                repo_dir: repo_path,
                file_path: req_path,
                follow_renames: follow,
                include_wip: true,
                limit: 500,
            };
            kagi_git::file_history(&req)
        });
        let view = view.downgrade();
        cx.spawn(async move |_app, acx| {
            let result = task.await.map_err(|e| e.to_string());
            let _ = view.update(acx, |v, cx| {
                v.seed_history(generation, result, origin, emit_loaded, cx)
            });
        })
        .detach();
    }

    /// Build the [`MainDiffView`] for the pane's currently-selected entry,
    /// reusing the existing diff renderer pipeline. The git diff computation
    /// (Backend open + per-file diff) runs **off the UI thread** via
    /// `cx.background_spawn` so selecting a row on a large file/repo can't jank
    /// the frame. Only the plain `kagi_git::FileDiff` crosses back; the row
    /// build + syntax highlight then run on the UI thread in the marshalled
    /// result, because `MainDiffView` / `DiffRow` hold GPUI types and are
    /// therefore `!Send`.
    ///
    /// The result is guarded by the pane's monotonic `req` token (bumped here
    /// on every call) plus the view's `generation`: a newer diff load (rapid
    /// arrowing / Refresh, including A→B→A) bumps `req`, and a history reload
    /// bumps `generation`, so a superseded load is discarded instead of
    /// overwriting the current selection's diff. Never panics on missing /
    /// binary / deleted files — falls back to `diff = None` so the banner
    /// messaging covers it.
    fn fh_load_diff(
        &mut self,
        view: &Entity<FileHistoryView>,
        repo_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = fh_diff_pane(view, cx) else {
            return;
        };
        let (entry, generation) = {
            let d = &view.read(cx).data;
            (d.selected_entry().cloned(), d.generation)
        };
        let Some(entry) = entry else {
            pane.update(cx, |p, cx| {
                p.diff = None;
                cx.notify();
            });
            return;
        };

        let path = entry.change.path_after.clone();
        let kind = entry.kind;
        let commit_id = entry.commit.as_ref().map(|c| CommitId(c.full_hash.clone()));

        // Bump the pane's monotonic per-diff token and capture it (plus the
        // view's `generation`) so the marshalled result is applied only if
        // neither a newer diff load nor a history reload has superseded it.
        let req = pane.update(cx, |p, _| {
            p.req = p.req.wrapping_add(1);
            p.req
        });

        // Off-thread: open the repo and compute the per-file diff (the expensive
        // I/O + diff work). `FileDiff` is plain `kagi_git` data (`Send`); the GPUI
        // view types are built on the UI thread in the marshalled result below.
        let diff_path = path.clone();
        let task = cx.background_spawn(async move {
            let repo = kagi_git::Backend::open(&repo_path).ok()?;
            let result = match kind {
                FileHistoryEntryKind::Wip => {
                    // Prefer the unstaged diff; fall back to staged (e.g. fully
                    // staged change) so the WIP entry still shows something.
                    match repo.unstaged_file_diff(&diff_path) {
                        Ok(d) if !d.hunks.is_empty() || d.is_binary => Ok(d),
                        _ => repo.staged_file_diff(&diff_path),
                    }
                }
                FileHistoryEntryKind::Commit => match commit_id {
                    Some(id) => repo.commit_file_diff(&id, &diff_path),
                    None => return None,
                },
            };
            Some(result.map_err(|e| e.to_string()))
        });

        let view = view.downgrade();
        let pane = pane.downgrade();
        cx.spawn(async move |_app, acx| {
            let result = task.await;
            let _ = pane.update(acx, |p, cx| {
                // Discard a superseded diff load (newer selection) or one
                // invalidated by a history reload (the view's generation moved,
                // or the view is already gone).
                let gen_ok = view
                    .upgrade()
                    .map(|v| v.read(cx).data.generation == generation)
                    .unwrap_or(false);
                if p.req != req || !gen_ok {
                    return;
                }
                let built = match result {
                    // T-WS-EDITOR-005 finding #10: shared builder (count →
                    // from_file_diff → stats → highlight → assemble), same
                    // pipeline as `EditorWorkspaceView`'s WIP-diff loader and
                    // `set_commit_main_diff`'s headless path.
                    Some(Ok(file_diff)) => Some(build_main_diff_view(
                        &file_diff,
                        &path,
                        0,
                        // The source is unused by the File History renderer;
                        // Unstaged carries the path for completeness.
                        MainDiffSource::Unstaged { path: path.clone() },
                    )),
                    Some(Err(e)) => {
                        klog!("file-history diff error: {}", e);
                        None
                    }
                    None => None,
                };
                p.diff = built;
                cx.notify();
            });
        })
        .detach();
    }
}
