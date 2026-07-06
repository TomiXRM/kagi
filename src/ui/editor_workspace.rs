//! Editor Workspace (T-WS-EDITOR-001 / ADR-0120 §4): the `WorkspaceMode::Editor`
//! body — left = working-tree changed-file tree, center = the selected file's
//! read-only code viewer, right = its WIP hunks.
//!
//! `EditorWorkspaceView` is a "fat" `Entity<T>` per the ADR-0117 template (same
//! shape as `FileHistoryView`): it holds `repo_path` and drives its own reads
//! off the `Backend` via `cx.background_spawn`, so entity-initiated actions
//! (tree click → select) update *self* and never re-enter `KagiApp` (which
//! would double-borrow the leased entity and panic). The only parent callback
//! is `close`, via the `WeakEntity<KagiApp>` back-ref — used ONLY from event
//! listeners, never in `Render`.
//!
//! v1 is read-only (T-WS-EDITOR-001 scope): no stage/discard/save affordance
//! lives here — Git writes stay behind the existing plan→confirm→execute
//! pipeline (invariant 4), and file *editing* is T-WS-EDITOR-002.

use gpui::WeakEntity;
use kagi_git::ChangeKind;

use super::commit_panel;
use super::file_tree::{self, TreeRow};
use super::i18n::Msg;
use super::render_helpers::*;
use super::*;

/// Files whose text content is skipped past this many lines (binary-ish /
/// pathologically large — same order of magnitude as gpui-component's
/// `code_editor` ~50K-line comfort zone). Shows the placeholder instead.
const MAX_EDITOR_LINES: usize = 50_000;

/// Fixed left tree-pane width. v1 has no drag-resize divider (YAGNI — add one
/// modeled on `file_history_geom` if users ask for it).
const TREE_PANE_W: f32 = 240.0;
/// Fixed right hunks-pane width.
const HUNKS_PANE_W: f32 = 380.0;

/// The Editor workspace view-model + entity (ADR-0117 fat-entity template).
pub struct EditorWorkspaceView {
    /// Weak back-ref to the parent, used ONLY in event listeners (close) —
    /// never read in a `Render` path.
    pub(crate) app: WeakEntity<super::KagiApp>,
    /// Repo root. Constant for the entity's life (dropped on repo/tab switch
    /// by `reset_per_repo_ui`, same as `FileHistoryView` / `EcosystemView`).
    pub(crate) repo_path: PathBuf,

    /// Working-tree changed files (staged + unstaged + untracked, merged and
    /// de-duplicated — see `merge_working_tree_files`).
    pub files: Vec<FileStatus>,
    /// Flattened tree rows built from `files` via `file_tree::build_file_tree`.
    pub tree: Vec<TreeRow>,
    /// `true` while the working-tree file-list load is in flight.
    pub loading: bool,
    /// Set if the file-list load failed.
    pub error: Option<String>,
    /// Monotonic generation for the file-list load; discards a superseded
    /// reload's result (defensive — v1 only loads once per open, but this
    /// keeps the entity robust against a future refresh action).
    generation: u64,

    /// Index into `files` of the selected row, if any.
    pub selected: Option<usize>,
    /// `true` while the selected file's content + diff load is in flight.
    pub file_loading: bool,
    /// The selected file's raw text, once loaded. `None` while loading, on a
    /// read error, or when the file is binary / over `MAX_EDITOR_LINES`
    /// (guarded — the center pane shows a placeholder instead).
    pub content: Option<String>,
    /// Highlighter language for `content` (`diff_view::lang_for_ext`, `"text"`
    /// fallback).
    pub content_lang: &'static str,
    /// The selected file looked binary (NUL byte in the leading probe).
    pub content_binary: bool,
    /// The selected file's line count exceeds `MAX_EDITOR_LINES`.
    pub content_too_large: bool,
    /// The selected file's WIP diff (unstaged, falling back to staged),
    /// reusing the existing diff-view pipeline — `None` while loading or if
    /// there is nothing to show (e.g. an untracked file with no diff).
    pub diff: Option<MainDiffView>,
    /// Monotonic per-file load token; discards a superseded content/diff load
    /// (rapid tree clicks).
    file_req: u64,

    /// The code viewer `InputState`, created lazily on first render (needs a
    /// `Window`, only available there — see `sync_editor`).
    pub editor: Option<Entity<InputState>>,
    /// Hash of the last `(path, content)` pushed into `editor`, so `sync_editor`
    /// only calls `set_value` when the content actually changed (guards
    /// against clobbering the viewer every frame — same technique as
    /// `ConflictEditorInputs.content_sig`, reusing `conflict_content_sig`).
    pushed_sig: u64,

    /// Scroll handle for the virtualized left tree list.
    pub tree_scroll: UniformListScrollHandle,
    /// Scroll handle for the right hunks list.
    pub diff_scroll: UniformListScrollHandle,
}

impl EditorWorkspaceView {
    fn new(app: WeakEntity<super::KagiApp>, repo_path: PathBuf) -> Self {
        Self {
            app,
            repo_path,
            files: Vec::new(),
            tree: Vec::new(),
            loading: false,
            error: None,
            generation: 0,
            selected: None,
            file_loading: false,
            content: None,
            content_lang: "text",
            content_binary: false,
            content_too_large: false,
            diff: None,
            file_req: 0,
            editor: None,
            pushed_sig: 0,
            tree_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
        }
    }

    /// Kick off the working-tree file-list load. Marshals the result back into
    /// *this* entity, guarded by `generation` so a superseded reload no-ops
    /// (and a dropped entity — `close_editor_workspace` — simply no-ops too,
    /// since `cx.spawn`'s `view` handle is weak).
    pub fn start_load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let repo_path = self.repo_path.clone();

        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&repo_path)
                .map_err(|e| e.to_string())
                .and_then(|b| b.working_tree_status().map_err(|e| e.to_string()))
        });

        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| {
                if v.generation != generation {
                    return;
                }
                v.loading = false;
                match result {
                    Ok(status) => {
                        let files = merge_working_tree_files(&status);
                        v.tree = file_tree::build_file_tree(&files);
                        klog!("editor-ws: files {}", files.len());
                        v.files = files;
                        if !v.files.is_empty() {
                            v.select(0, cx);
                        }
                    }
                    Err(e) => v.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Select a tree row's file (index into `files`) and kick off its
    /// content + diff load.
    pub fn select(&mut self, file_index: usize, cx: &mut Context<Self>) {
        let Some(file) = self.files.get(file_index) else {
            return;
        };
        self.selected = Some(file_index);
        self.content = None;
        self.content_binary = false;
        self.content_too_large = false;
        self.diff = None;
        klog!("editor-ws: file {}", file.path.display());
        self.load_selected(cx);
        cx.notify();
    }

    /// Load the selected file's raw text (off-thread, guarded by
    /// `MAX_EDITOR_LINES` + a binary probe) and its WIP diff (unstaged,
    /// falling back to staged — mirrors `FileHistoryView::load_diff`).
    fn load_selected(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.selected else { return };
        let Some(path) = self.files.get(idx).map(|f| f.path.clone()) else {
            return;
        };
        self.file_loading = true;
        self.file_req = self.file_req.wrapping_add(1);
        let file_req = self.file_req;
        let generation = self.generation;
        let repo_path = self.repo_path.clone();
        let bg_path = path.clone();

        let task = cx.background_spawn(async move {
            let bytes = std::fs::read(repo_path.join(&bg_path)).ok();
            let is_binary = bytes
                .as_deref()
                .map(kagi_domain::checklist::content_looks_binary)
                .unwrap_or(false);
            let text = if is_binary {
                None
            } else {
                bytes.and_then(|b| String::from_utf8(b).ok())
            };

            let diff = kagi_git::Backend::open(&repo_path).ok().and_then(|repo| {
                match repo.unstaged_file_diff(&bg_path) {
                    Ok(d) if !d.hunks.is_empty() || d.is_binary => Some(d),
                    _ => repo.staged_file_diff(&bg_path).ok(),
                }
            });
            (text, is_binary, diff)
        });

        cx.spawn(async move |view, acx| {
            let (text, is_binary, file_diff) = task.await;
            let _ = view.update(acx, |v, cx| {
                if v.file_req != file_req || v.generation != generation {
                    return;
                }
                v.file_loading = false;
                v.content_binary = is_binary;
                let too_large = text
                    .as_ref()
                    .map(|t| t.lines().count() > MAX_EDITOR_LINES)
                    .unwrap_or(false);
                v.content_too_large = too_large;
                v.content = if too_large { None } else { text };
                v.content_lang = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(lang_for_ext)
                    .unwrap_or("text");
                v.diff = file_diff.map(|d| build_wip_diff_view(&d, &path));
                cx.notify();
            });
        })
        .detach();
    }

    /// Lazily create the code-viewer `InputState` (needs a `Window`, only
    /// available in `Render`) and push the selected file's content into it,
    /// guarded by a content-hash sig so a re-render that changed nothing
    /// doesn't clobber the viewer (scroll/selection would reset otherwise).
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.content.clone() else {
            return;
        };
        if self.editor.is_none() {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor(self.content_lang)
                    .line_number(true)
            });
            self.editor = Some(state);
        }
        let path = self
            .selected
            .and_then(|i| self.files.get(i))
            .map(|f| &f.path);
        let sig = path
            .map(|p| conflict_content_sig(p, &content, false))
            .unwrap_or(0);
        if sig == self.pushed_sig {
            return;
        }
        self.pushed_sig = sig;
        let lang = self.content_lang;
        if let Some(editor) = self.editor.clone() {
            editor.update(cx, |s, cx| {
                s.set_highlighter(lang, cx);
                s.set_value(content, window, cx);
            });
        }
    }

    /// Ask the parent to close this view (drops the entity + resets
    /// `workspace_mode`). Safe per ADR-0117: only clears fields, never
    /// re-leases this entity.
    fn request_close(&self, cx: &mut Context<Self>) {
        let _ = self.app.update(cx, |app, cx| {
            app.close_editor_workspace();
            cx.notify();
        });
    }
}

/// Merge working-tree `staged` + `unstaged` + `untracked` into one
/// de-duplicated `Vec<FileStatus>` for the tree (T-WS-EDITOR-001 v1 data
/// source — conflicted files are left to the dedicated Conflict Mode UI, out
/// of scope here). Unstaged wins over staged for a path present in both (it's
/// the more "current" working-tree state); untracked files are synthesized as
/// `Added`.
fn merge_working_tree_files(status: &kagi_git::WorkingTreeStatus) -> Vec<FileStatus> {
    let mut files: Vec<FileStatus> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for f in status.unstaged.iter().chain(status.staged.iter()) {
        if seen.insert(f.path.clone()) {
            files.push(f.clone());
        }
    }
    for p in &status.untracked {
        if seen.insert(p.clone()) {
            files.push(FileStatus {
                path: p.clone(),
                change: ChangeKind::Added,
            });
        }
    }
    files
}

/// Build the right pane's `MainDiffView` from a raw `FileDiff`, reusing the
/// existing diff-view pipeline (`FileDiffView::from_file_diff` +
/// `highlight_diff_rows`) exactly like `FileHistoryView::load_diff` does.
fn build_wip_diff_view(file_diff: &kagi_git::FileDiff, path: &Path) -> MainDiffView {
    let added: usize = file_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind == DiffLineKind::Added)
        .count();
    let removed: usize = file_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind == DiffLineKind::Removed)
        .count();
    let fdv = FileDiffView::from_file_diff(file_diff, 0);
    let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
    let mut rows = fdv.rows;
    let _ = highlight_diff_rows(&mut rows, path);
    MainDiffView {
        title: fdv.file_name,
        stats,
        rows,
        source: MainDiffSource::Unstaged {
            path: path.to_path_buf(),
        },
    }
}

impl Render for EditorWorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_editor(window, cx);
        render_editor_workspace(self, cx)
    }
}

// ── KagiApp entry points (ADR-0117 / ADR-0120) ─────────────────────

impl super::KagiApp {
    /// Open the Editor workspace for the current repo and switch
    /// `workspace_mode` to `Editor`. No-op when no repository is open.
    pub fn open_editor_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let weak = cx.weak_entity();
        let view = cx.new(|_| EditorWorkspaceView::new(weak, repo_path));
        self.editor_workspace = Some(view.clone());
        self.workspace_mode = workspace::WorkspaceMode::Editor;
        klog!("editor-ws: open");
        view.update(cx, |v, cx| v.start_load(cx));
        cx.notify();
    }

    /// Close the Editor workspace (drops the entity) and switch
    /// `workspace_mode` back to `Graph`.
    pub fn close_editor_workspace(&mut self) {
        self.editor_workspace = None;
        self.workspace_mode = workspace::WorkspaceMode::Graph;
    }
}

// ──────────────────────────────────────────────────────────────
// Rendering
// ──────────────────────────────────────────────────────────────

/// Render the whole Editor workspace: header + left file tree + center code
/// viewer + right hunks. Returns the body fragment `render_body` drops in
/// place of the normal sidebar+center+right area (see the `CenterPane::Editor`
/// arm in `render_body.rs`).
fn render_editor_workspace(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let selected_path = view
        .selected
        .and_then(|i| view.files.get(i))
        .map(|f| SharedString::from(f.path.to_string_lossy().into_owned()));

    let close = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.request_close(cx);
    });
    let header = div()
        .id("ews-header")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .px_3()
        .py_1()
        .gap_2()
        .bg(rgb(theme().surface))
        .child(
            div()
                .id("ews-close")
                .px_2()
                .py_px()
                .rounded_sm()
                .cursor_pointer()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .hover(|s| s.bg(rgb(theme().selected)))
                .on_click(close)
                .child(SharedString::from("\u{2190} Graph")),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from("Editor Workspace")),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .children(selected_path),
        );

    let divider = || {
        div()
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
    };

    let body = div()
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .child(render_tree_pane(view, cx))
        .child(divider())
        .child(render_center_pane(view, cx))
        .child(divider())
        .child(render_hunks_pane(view, cx));

    div()
        .id("editor-workspace")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .h_full()
        .child(header)
        .child(body)
        .into_any_element()
}

/// Left pane: the virtualized, clickable working-tree file tree.
fn render_tree_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let mut pane = div()
        .w(theme::scaled_px(TREE_PANE_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel));

    if view.loading {
        return pane
            .child(placeholder_text(Msg::EditorWorkspaceLoading.t()))
            .into_any_element();
    }
    if let Some(err) = view.error.clone() {
        return pane.child(placeholder_text(err)).into_any_element();
    }
    if view.files.is_empty() {
        return pane
            .child(placeholder_text(Msg::EditorWorkspaceEmpty.t()))
            .into_any_element();
    }

    let row_count = view.tree.len();
    let scroll_handle = view.tree_scroll.clone();
    let scrollbar_handle = scroll_handle.clone();
    let list = with_vertical_scrollbar(
        "ews-tree-scroll",
        &scrollbar_handle,
        uniform_list(
            "ews-tree-list",
            row_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|i| render_tree_row(this, i, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(scroll_handle)
        .flex_1()
        .min_h(px(0.)),
        false,
    );
    pane = pane.child(list);
    pane.into_any_element()
}

/// One row in the left file tree (dir label or a clickable file row).
fn render_tree_row(
    view: &EditorWorkspaceView,
    index: usize,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<gpui::AnyElement> {
    let row = view.tree.get(index)?.clone();
    match row {
        TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(("ews-dir", index))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name)
                    .into_any_element(),
            )
        }
        TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let (badge, badge_color, _) = commit_panel::status_badge(&change, false);
            let is_selected = view.selected == Some(file_index);
            let row_bg = if is_selected {
                theme().selected
            } else if index % 2 == 1 {
                theme().bg_row_alt
            } else {
                theme().panel
            };
            let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.select(file_index, cx);
            });
            Some(
                div()
                    .id(("ews-file", file_index))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(theme::scaled_px(8.0 + indent))
                    .pr(theme::scaled_px(4.0))
                    .py_px()
                    .bg(rgb(row_bg))
                    .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
                    .on_click(click)
                    .cursor_pointer()
                    .child(
                        div()
                            .w(theme::scaled_px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .truncate()
                            .child(name),
                    )
                    .into_any_element(),
            )
        }
    }
}

/// Center pane: the selected file's read-only code viewer, or a placeholder.
fn render_center_pane(
    view: &EditorWorkspaceView,
    _cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let mut pane = div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_base));

    let placeholder = if view.loading {
        Some(Msg::EditorWorkspaceLoading.t())
    } else if view.selected.is_none() {
        Some(Msg::EditorWorkspaceSelectFile.t())
    } else if view.file_loading {
        Some(Msg::EditorWorkspaceLoading.t())
    } else if view.content_binary {
        Some(Msg::EditorWorkspaceBinary.t())
    } else if view.content_too_large {
        Some(Msg::EditorWorkspaceTooLarge.t())
    } else if view.content.is_none() {
        Some(Msg::EditorWorkspaceSelectFile.t())
    } else {
        None
    };

    if let Some(msg) = placeholder {
        pane = pane.child(placeholder_text(msg));
        return pane.into_any_element();
    }

    let Some(editor) = view.editor.clone() else {
        return pane.into_any_element();
    };
    pane.child(
        div()
            .id("ews-editor")
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .child(
                Input::new(&editor)
                    .disabled(true)
                    .appearance(true)
                    .bordered(false)
                    .h_full(),
            ),
    )
    .into_any_element()
}

/// Right pane: the selected file's WIP hunks via the generic diff-list
/// renderer, or a placeholder when there is nothing to show.
fn render_hunks_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let mut pane = div()
        .w(theme::scaled_px(HUNKS_PANE_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel));

    match view.diff.clone() {
        Some(diff) => {
            let scroll = view.diff_scroll.clone();
            pane = pane.child(render_diff_list::<EditorWorkspaceView>(
                diff, None, None, scroll, cx,
            ));
        }
        None => {
            let msg = if view.file_loading || view.loading {
                Msg::EditorWorkspaceLoading.t()
            } else {
                Msg::EditorWorkspaceNoDiff.t()
            };
            pane = pane.child(placeholder_text(msg));
        }
    }
    pane.into_any_element()
}

/// A centered single-line placeholder message, reused by all three panes.
fn placeholder_text(msg: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .p_3()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(msg.into())
}
