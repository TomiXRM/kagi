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
//!
//! T-WS-EDITOR-004 (user feedback round 2) adds: a `TreeSource` toggle
//! (Changes ⇄ All files, with an auto-switch to All on a clean worktree so
//! the workspace isn't a dead end), drag-resizable tree/hunks panes, and a
//! header toolbar button.

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

/// Default left tree-pane width (drag-resizable via `DividerKind::EditorTree`,
/// T-WS-EDITOR-004 — clamped to `EDITOR_TREE_MIN..EDITOR_TREE_MAX` in
/// `render_divider.rs`).
const TREE_PANE_DEFAULT_W: f32 = 240.0;
/// Default right hunks-pane width (drag-resizable via `DividerKind::EditorHunks`).
const HUNKS_PANE_DEFAULT_W: f32 = 380.0;

/// What the left tree pane lists (T-WS-EDITOR-004 user feedback: "I also want
/// to use this as a normal tree view/editor, not just Changes").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TreeSource {
    /// Working-tree changed files only (v1 behaviour).
    #[default]
    Changes,
    /// Every tracked + untracked (non-ignored) file, with changed ones still
    /// badged (`Backend::worktree_files` merged against `working_tree_status`).
    All,
}

/// A file entry in the workspace tree: repo-relative path + its change kind,
/// if any. `change` is `None` for an unmodified file in `TreeSource::All` — a
/// `FileStatus` can't represent "no change", so this shared shape lets both
/// tree sources use the same `files`/`tree`/selection plumbing below.
#[derive(Clone, Debug)]
pub struct WorkspaceFile {
    pub path: PathBuf,
    pub change: Option<ChangeKind>,
}

/// The Editor workspace view-model + entity (ADR-0117 fat-entity template).
pub struct EditorWorkspaceView {
    /// Weak back-ref to the parent, used ONLY in event listeners (close) —
    /// never read in a `Render` path.
    pub(crate) app: WeakEntity<super::KagiApp>,
    /// Repo root. Constant for the entity's life (dropped on repo/tab switch
    /// by `reset_per_repo_ui`, same as `FileHistoryView` / `EcosystemView`).
    pub(crate) repo_path: PathBuf,

    /// Which files `files`/`tree` are currently listing (T-WS-EDITOR-004).
    pub source: TreeSource,
    /// Working-tree files for the active `source`: `Changes` is staged +
    /// unstaged + untracked (merged and de-duplicated — see
    /// `merge_working_tree_files`); `All` is every tracked + untracked file
    /// with changed ones still badged (see `merge_all_files`).
    pub files: Vec<WorkspaceFile>,
    /// Flattened tree rows built from `files` via `file_tree::build_file_tree_opt`.
    pub tree: Vec<TreeRow>,
    /// Left tree-pane width (drag-resizable, T-WS-EDITOR-004).
    pub tree_w: f32,
    /// Right hunks-pane width (drag-resizable, T-WS-EDITOR-004).
    pub hunks_w: f32,
    /// Collapsed directory rows, keyed by their index into `tree` (stable per
    /// load — `tree` is only rebuilt by `start_load`, which clears this).
    /// Children of a collapsed dir are filtered out by `visible_tree_indices`.
    pub collapsed: HashSet<usize>,
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
            source: TreeSource::default(),
            files: Vec::new(),
            tree: Vec::new(),
            tree_w: TREE_PANE_DEFAULT_W,
            hunks_w: HUNKS_PANE_DEFAULT_W,
            collapsed: HashSet::new(),
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

    /// Kick off the file-list load for the current `source`. Marshals the
    /// result back into *this* entity, guarded by `generation` so a
    /// superseded reload no-ops (and a dropped entity —
    /// `close_editor_workspace` — simply no-ops too, since `cx.spawn`'s `view`
    /// handle is weak). Bumping `generation` on every call also makes a
    /// `TreeSource` switch mid-load safe: the stale load's result is dropped.
    pub fn start_load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let repo_path = self.repo_path.clone();
        let source = self.source;

        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&repo_path).map_err(|e| e.to_string())?;
            let status = backend.working_tree_status().map_err(|e| e.to_string())?;
            let changed = merge_working_tree_files(&status);
            match source {
                TreeSource::Changes => Ok(changed),
                TreeSource::All => {
                    let all_paths = backend.worktree_files().map_err(|e| e.to_string())?;
                    Ok(merge_all_files(all_paths, &changed))
                }
            }
        });

        cx.spawn(async move |view, acx| {
            let result: Result<Vec<WorkspaceFile>, String> = task.await;
            let _ = view.update(acx, |v, cx| {
                if v.generation != generation {
                    return;
                }
                v.loading = false;
                match result {
                    Ok(files) => {
                        let prev_selected_path = v
                            .selected
                            .and_then(|i| v.files.get(i))
                            .map(|f| f.path.clone());
                        v.tree = build_workspace_tree(&files);
                        // Collapse indices key into the OLD tree — reset.
                        v.collapsed.clear();
                        klog!("editor-ws: files {}", files.len());
                        let was_empty = files.is_empty();
                        v.files = files;

                        // T-WS-EDITOR-004 user feedback #1: a clean worktree
                        // has nothing to show in Changes mode on the initial
                        // open — auto-switch to All so the workspace isn't a
                        // dead end. Gated to `generation == 1` (the very first
                        // load) so manually toggling back to Changes later
                        // doesn't bounce right back to All.
                        if generation == 1 && v.source == TreeSource::Changes && was_empty {
                            cx.notify();
                            v.switch_source(TreeSource::All, cx);
                            return;
                        }

                        // Keep the selection if the file is still listed
                        // (source toggle / reload); else select the first
                        // file, or clear selection if the list is empty.
                        let restore = prev_selected_path
                            .and_then(|p| v.files.iter().position(|f| f.path == p));
                        match restore.or(if v.files.is_empty() { None } else { Some(0) }) {
                            Some(i) => v.select(i, cx),
                            None => v.selected = None,
                        }
                    }
                    Err(e) => v.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Switch the tree source and reload (T-WS-EDITOR-004: the Changes/All
    /// chips, and the clean-worktree auto-switch in `start_load`). No-ops if
    /// already on `source`.
    pub fn set_source(&mut self, source: TreeSource, cx: &mut Context<Self>) {
        if self.source == source {
            return;
        }
        self.switch_source(source, cx);
    }

    /// Shared by the manual toggle and the auto-switch: set `source`, emit
    /// the contract log line, and reload.
    fn switch_source(&mut self, source: TreeSource, cx: &mut Context<Self>) {
        self.source = source;
        klog!(
            "editor-ws: source {}",
            match source {
                TreeSource::Changes => "changes",
                TreeSource::All => "all",
            }
        );
        self.start_load(cx);
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

    /// Step the file selection up/down (root ↑/↓ handler, T-WS-EDITOR-001
    /// user feedback). Walks the *visible* file rows (dirs and collapsed
    /// subtrees skipped) so the selection never lands on a hidden file, and
    /// scrolls the tree to keep the selected row on screen.
    pub fn step_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let visible = visible_tree_indices(&self.tree, &self.collapsed);
        // (position in the visible list, file_index) for every visible file row.
        let file_rows: Vec<(usize, usize)> = visible
            .iter()
            .enumerate()
            .filter_map(|(vis_pos, &ti)| match self.tree.get(ti) {
                Some(TreeRow::File { file_index, .. }) => Some((vis_pos, *file_index)),
                _ => None,
            })
            .collect();
        if file_rows.is_empty() {
            return;
        }
        let cur = self
            .selected
            .and_then(|sel| file_rows.iter().position(|&(_, fi)| fi == sel));
        let next = match cur {
            Some(p) => (p as i64 + i64::from(delta)).clamp(0, file_rows.len() as i64 - 1) as usize,
            // No (visible) selection yet: ↓ starts at the top, ↑ at the bottom.
            None if delta >= 0 => 0,
            None => file_rows.len() - 1,
        };
        let (vis_pos, file_index) = file_rows[next];
        if self.selected == Some(file_index) {
            return;
        }
        // gpui 0.2.2 has no `Nearest`; `Center` matches the commit list's
        // jump behaviour (`mod.rs` scroll_to_item call sites).
        self.tree_scroll
            .scroll_to_item(vis_pos, ScrollStrategy::Center);
        self.select(file_index, cx);
    }

    /// Toggle a directory row's collapsed state (Zed-style chevron click).
    /// `tree_index` keys into `self.tree` (the unfiltered base rows).
    pub fn toggle_dir(&mut self, tree_index: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&tree_index) {
            self.collapsed.insert(tree_index);
        }
        cx.notify();
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

/// Indices into `tree` that are visible given the collapsed dir set: children
/// of a collapsed dir (every following row strictly deeper than it) are
/// skipped, Zed-style. Pure — unit-tested below.
fn visible_tree_indices(tree: &[TreeRow], collapsed: &HashSet<usize>) -> Vec<usize> {
    let mut out = Vec::with_capacity(tree.len());
    // While `Some(d)`, rows deeper than `d` are hidden (inside a collapsed dir).
    let mut hide_deeper_than: Option<usize> = None;
    for (i, row) in tree.iter().enumerate() {
        let (depth, is_dir) = match row {
            TreeRow::Dir { depth, .. } => (*depth, true),
            TreeRow::File { depth, .. } => (*depth, false),
        };
        if let Some(d) = hide_deeper_than {
            if depth > d {
                continue;
            }
            hide_deeper_than = None;
        }
        out.push(i);
        if is_dir && collapsed.contains(&i) {
            hide_deeper_than = Some(depth);
        }
    }
    out
}

/// Merge working-tree `staged` + `unstaged` + `untracked` into one
/// de-duplicated `Vec<WorkspaceFile>` for the tree (`TreeSource::Changes` —
/// conflicted files are left to the dedicated Conflict Mode UI, out of scope
/// here). Unstaged wins over staged for a path present in both (it's the more
/// "current" working-tree state); untracked files are synthesized as `Added`.
fn merge_working_tree_files(status: &kagi_git::WorkingTreeStatus) -> Vec<WorkspaceFile> {
    let mut files: Vec<WorkspaceFile> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for f in status.unstaged.iter().chain(status.staged.iter()) {
        if seen.insert(f.path.clone()) {
            files.push(WorkspaceFile {
                path: f.path.clone(),
                change: Some(f.change.clone()),
            });
        }
    }
    for p in &status.untracked {
        if seen.insert(p.clone()) {
            files.push(WorkspaceFile {
                path: p.clone(),
                change: Some(ChangeKind::Added),
            });
        }
    }
    files
}

/// Merge the full tracked+untracked path list (`Backend::worktree_files`)
/// with the working-tree change kinds (by path) for `TreeSource::All` — an
/// unmodified file simply gets `change: None` (no badge), while a changed one
/// keeps its real `ChangeKind` so the badge still shows (T-WS-EDITOR-004
/// scope item 3).
fn merge_all_files(all_paths: Vec<PathBuf>, changed: &[WorkspaceFile]) -> Vec<WorkspaceFile> {
    let changes: HashMap<&Path, ChangeKind> = changed
        .iter()
        .filter_map(|f| f.change.clone().map(|c| (f.path.as_path(), c)))
        .collect();
    all_paths
        .into_iter()
        .map(|path| {
            let change = changes.get(path.as_path()).cloned();
            WorkspaceFile { path, change }
        })
        .collect()
}

/// Build the tree rows for `files`, sharing `file_tree`'s compression
/// algorithm via `build_file_tree_opt` (no duplicated tree logic between the
/// `Changes`/`All` sources — see `file_tree.rs`).
fn build_workspace_tree(files: &[WorkspaceFile]) -> Vec<TreeRow> {
    let pairs: Vec<(PathBuf, Option<ChangeKind>)> = files
        .iter()
        .map(|f| (f.path.clone(), f.change.clone()))
        .collect();
    file_tree::build_file_tree_opt(&pairs)
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

    /// Root ↑/↓ handler branch for the Editor workspace: step the file
    /// selection on the entity (normal parent→child `update`; the entity is
    /// not leased here).
    pub fn step_editor_ws_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(ev) = self.editor_workspace.clone() {
            ev.update(cx, |v, cx| v.step_selection(delta, cx));
        }
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

    // T-WS-EDITOR-004: drag-resizable, same shape as `divider1` in
    // `render_body.rs` — hover highlight + col-resize cursor + a
    // `DividerGhost` drag payload; `handle_divider_drag` (render_divider.rs)
    // does the actual width math per `DividerKind`.
    let tree_divider = div()
        .id("ews-divider-tree")
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::EditorTree,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );
    let hunks_divider = div()
        .id("ews-divider-hunks")
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::EditorHunks,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let body = div()
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .child(render_tree_pane(view, cx))
        .child(tree_divider)
        .child(render_center_pane(view, cx))
        .child(hunks_divider)
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

/// One "Changes"/"All" tab in the tree-source chip row (T-WS-EDITOR-004).
fn render_source_chip(
    id: &'static str,
    label: &'static str,
    active: bool,
    source: TreeSource,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.set_source(source, cx);
    });
    div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .py_px()
        .rounded_sm()
        .cursor_pointer()
        .text_xs()
        .text_color(rgb(if active {
            theme().text_main
        } else {
            theme().text_muted
        }))
        .bg(rgb(if active {
            theme().selected
        } else {
            theme().panel
        }))
        .when(!active, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        .on_click(click)
        .child(SharedString::from(label))
        .into_any_element()
}

/// The Changes/All tree-source toggle row, pinned above the tree list
/// (T-WS-EDITOR-004 user feedback #1).
fn render_source_chips(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    div()
        .id("ews-source-chips")
        .flex()
        .flex_row()
        .flex_shrink_0()
        .gap_1()
        .px_2()
        .py_1()
        .bg(rgb(theme().panel))
        .child(render_source_chip(
            "ews-source-changes",
            Msg::EditorWorkspaceSourceChanges.t(),
            view.source == TreeSource::Changes,
            TreeSource::Changes,
            cx,
        ))
        .child(render_source_chip(
            "ews-source-all",
            Msg::EditorWorkspaceSourceAll.t(),
            view.source == TreeSource::All,
            TreeSource::All,
            cx,
        ))
        .into_any_element()
}

/// Left pane: the Changes/All source chips + the virtualized, clickable
/// working-tree file tree.
fn render_tree_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let mut pane = div()
        .w(theme::scaled_px(view.tree_w))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        .child(render_source_chips(view, cx));

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

    // Zed-style collapse: the uniform_list virtualizes the *visible* rows;
    // the processor maps a visible position back to its base tree index.
    let visible = visible_tree_indices(&view.tree, &view.collapsed);
    let row_count = visible.len();
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
                    .filter_map(|i| render_tree_row(this, *visible.get(i)?, cx))
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
            // Zed-style collapsible dir row: chevron + name, click toggles.
            let indent = (depth as f32) * 12.0;
            let is_collapsed = view.collapsed.contains(&index);
            let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.toggle_dir(index, cx);
            });
            Some(
                div()
                    .id(("ews-dir", index))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(click)
                    .child(
                        div()
                            .w(theme::scaled_px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(if is_collapsed {
                                "\u{25b8}" // ▸
                            } else {
                                "\u{25be}" // ▾
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(theme().change_dir))
                            .truncate()
                            .child(name),
                    )
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
            let (badge, badge_color, _) = commit_panel::status_badge(change.as_ref(), false);
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
                // `disabled` is the only read-only gate gpui-component 0.5.1
                // offers, but its appearance paints the muted (gray) disabled
                // background — user report: the viewer looked like a disabled
                // form field. `appearance(false)` drops the component's own
                // bg/border so the pane's dark `bg_base` shows through, like a
                // real code viewer.
                Input::new(&editor)
                    .disabled(true)
                    .appearance(false)
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
        .w(theme::scaled_px(view.hunks_w))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(depth: usize, name: &str) -> TreeRow {
        TreeRow::Dir {
            depth,
            name: SharedString::from(name.to_string()),
        }
    }
    fn file(depth: usize, name: &str, file_index: usize) -> TreeRow {
        TreeRow::File {
            depth,
            name: SharedString::from(name.to_string()),
            file_index,
            change: Some(ChangeKind::Modified),
        }
    }

    /// src/(a.rs, ui/(b.rs)), root.rs — the shape `build_file_tree` emits.
    fn sample_tree() -> Vec<TreeRow> {
        vec![
            dir(0, "src"),         // 0
            file(1, "a.rs", 0),    // 1
            dir(1, "ui"),          // 2
            file(2, "b.rs", 1),    // 3
            file(0, "root.rs", 2), // 4
        ]
    }

    #[test]
    fn no_collapse_shows_everything() {
        let tree = sample_tree();
        assert_eq!(
            visible_tree_indices(&tree, &HashSet::new()),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn collapsing_a_dir_hides_its_subtree_only() {
        let tree = sample_tree();
        // Collapse src (idx 0): hides a.rs, ui, b.rs — root.rs stays.
        let collapsed: HashSet<usize> = [0].into_iter().collect();
        assert_eq!(visible_tree_indices(&tree, &collapsed), vec![0, 4]);
        // Collapse the nested ui (idx 2) only: hides b.rs.
        let collapsed: HashSet<usize> = [2].into_iter().collect();
        assert_eq!(visible_tree_indices(&tree, &collapsed), vec![0, 1, 2, 4]);
    }

    #[test]
    fn collapsed_state_of_a_hidden_dir_is_inert() {
        let tree = sample_tree();
        // src collapsed AND the hidden ui collapsed — same as src alone.
        let collapsed: HashSet<usize> = [0, 2].into_iter().collect();
        assert_eq!(visible_tree_indices(&tree, &collapsed), vec![0, 4]);
    }

    // ── T-WS-EDITOR-004: TreeSource::All merge logic ───────────────────────

    #[test]
    fn merge_all_files_badges_changed_and_blanks_unmodified() {
        let changed = vec![WorkspaceFile {
            path: PathBuf::from("src/a.rs"),
            change: Some(ChangeKind::Modified),
        }];
        let all = vec![PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")];

        let merged = merge_all_files(all, &changed);

        assert_eq!(merged.len(), 2);
        let a = merged
            .iter()
            .find(|f| f.path == Path::new("src/a.rs"))
            .unwrap();
        assert_eq!(a.change, Some(ChangeKind::Modified));
        let b = merged
            .iter()
            .find(|f| f.path == Path::new("src/b.rs"))
            .unwrap();
        assert_eq!(b.change, None);
    }

    #[test]
    fn merge_all_files_empty_changed_list_blanks_everything() {
        let all = vec![PathBuf::from("only.txt")];
        let merged = merge_all_files(all, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].change, None);
    }

    #[test]
    fn build_workspace_tree_shares_compression_with_option_changes() {
        // Mirrors file_tree's own single-child-compression test, but through
        // the WorkspaceFile/Option<ChangeKind> path — confirms build_workspace_tree
        // delegates to the same DirNode algorithm rather than a duplicate.
        let files = vec![
            WorkspaceFile {
                path: PathBuf::from("a/b/c.rs"),
                change: None,
            },
            WorkspaceFile {
                path: PathBuf::from("top.txt"),
                change: Some(ChangeKind::Added),
            },
        ];
        let rows = build_workspace_tree(&files);
        assert_eq!(rows.len(), 3); // Dir("a/b") + File(c.rs) + File(top.txt)
        assert!(rows
            .iter()
            .any(|r| matches!(r, TreeRow::Dir { name, .. } if name.as_ref() == "a/b")));
    }
}
