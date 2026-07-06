//! Editor Workspace (T-WS-EDITOR-001 / ADR-0120 §4): the Graph ⇄ Editor mode's
//! Editor body — left = working-tree changed-file tree, center = the selected
//! file's read-only code viewer, right = its WIP hunks. The mode is derived as
//! `editor_workspace.is_some()` rather than tracked separately
//! (T-WS-EDITOR-005 finding #11).
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
use gpui_component::input::InputEvent;
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

/// Files whose byte size exceeds this are skipped WITHOUT reading them at all
/// (T-WS-EDITOR-005 finding #2 — OOM guard: the previous code read the whole
/// file into memory before checking anything). Checked via `fs::metadata`
/// first; `MAX_EDITOR_LINES` is still enforced afterward for files that pass
/// this gate but are pathologically long-and-thin.
const MAX_EDITOR_BYTES: u64 = 10 * 1024 * 1024;

/// How many leading bytes of a file are probed for "looks binary" (NUL byte).
/// Probing only this slice — not the whole file — avoids scanning a large
/// text file just to prove it isn't binary (T-WS-EDITOR-005 finding #2).
const BINARY_PROBE_BYTES: usize = 8 * 1024;

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
    /// The selected file's line count (or byte size) exceeds the guard
    /// consts. `content` is `None` when this is set.
    pub content_too_large: bool,
    /// `fs::read` failed for the selected file — either it was deleted from
    /// the working tree, or the file's `ChangeKind` says `Deleted` outright
    /// (T-WS-EDITOR-005 finding #6). Distinct from `content_binary` /
    /// `content_too_large` so the center pane can show an accurate
    /// placeholder instead of the generic "select a file" message.
    pub content_missing: bool,
    /// The selected file's bytes aren't valid UTF-8 (and weren't flagged
    /// binary by the NUL-byte probe — e.g. a Shift-JIS-encoded text file;
    /// T-WS-EDITOR-005 finding #6).
    pub content_undecodable: bool,
    /// Hash of `(path, content)` for the currently loaded `content`, computed
    /// once when the load completes (T-WS-EDITOR-005 finding #9) rather than
    /// every render — `sync_editor` compares this against `pushed_sig`
    /// without touching `content` at all unless it actually needs to push.
    content_sig: u64,
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
    /// `true` once `editor`'s live text differs from `content` (the last
    /// loaded/saved snapshot) — set by the `InputEvent::Change` subscription
    /// in `sync_editor` (T-WS-EDITOR-002). Drives the `●` dirty indicators,
    /// the Cmd-S save gate, and the unsaved-changes guard on file/source
    /// switch and close.
    ///
    /// The subscription compares the editor's current text against
    /// `content` rather than using a synchronous "am I mid-push" flag: a
    /// gpui-component `set_value` push ALSO fires `InputEvent::Change`, but
    /// `cx.emit` queues it as a deferred effect (`Context::emit` pushes onto
    /// `pending_effects`) — it is delivered after `sync_editor` returns, by
    /// which point a plain bool flag set/cleared synchronously around the
    /// push would already be back to its resting value. Comparing text is
    /// immune to that ordering.
    pub dirty: bool,
    /// `true` when the FS watcher observed a working-tree change while
    /// `dirty` was set (T-WS-EDITOR-002 §4) — the buffer was NOT
    /// auto-reloaded (that would clobber the unsaved edit), so a banner asks
    /// the user to reload or keep editing. Cleared on save, reload, or
    /// switching away from the file.
    pub external_changed: bool,

    /// Scroll handle for the virtualized left tree list.
    pub tree_scroll: UniformListScrollHandle,
    /// Scroll handle for the right hunks list.
    pub diff_scroll: UniformListScrollHandle,

    /// Whether the left tree pane renders (T-WS-EDITOR-005 finding #3).
    /// Pushed by `render_body` from `workspace::resolve_workspace`'s
    /// `layout.left` right before this entity is embedded — the resolver's
    /// output stays the single source of truth for View → Toggle Sidebar even
    /// though this entity self-renders and can't be skipped slot-by-slot the
    /// way the sidebar/right-panel arms are. Defaults to `true` so the tree
    /// shows before the first `render_body` push.
    pub show_tree: bool,
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
            content_missing: false,
            content_undecodable: false,
            content_sig: 0,
            diff: None,
            file_req: 0,
            editor: None,
            pushed_sig: 0,
            dirty: false,
            external_changed: false,
            tree_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            show_tree: true,
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
                        // Collapse indices key into the OLD tree — reset to
                        // the source's default: Changes opens fully expanded
                        // (small, curated set), All opens fully collapsed
                        // (whole worktree — user request).
                        v.collapsed = match v.source {
                            TreeSource::Changes => HashSet::new(),
                            TreeSource::All => all_dir_indices(&v.tree),
                        };
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
                        // T-WS-EDITOR-002 §4: a watcher-driven reload
                        // (`on_worktree_changed`) must NOT clobber a dirty
                        // buffer — `select` unconditionally resets
                        // `content`/`diff`/`dirty`. The save path clears
                        // `dirty` itself before calling `start_load`, so this
                        // only applies to a genuinely unsaved external-change
                        // race.
                        match restore.or(if v.files.is_empty() { None } else { Some(0) }) {
                            Some(i) if preserve_dirty_selection(v.dirty, restore, i) => {
                                // Same dirty file, possibly renumbered by the
                                // reload — keep the tree highlight / dirty
                                // dot on the right row without touching the
                                // buffer.
                                v.selected = Some(i);
                            }
                            Some(_) if v.dirty => {
                                // The dirty file fell out of the fresh list
                                // (e.g. its status changed) — never silently
                                // discard the edit; leave the stale
                                // selection/content as-is.
                            }
                            Some(i) => v.select(i, cx),
                            None if v.dirty => {}
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

    /// Shared by `request_switch_source` (T-WS-EDITOR-002 dirty-guarded chip
    /// click) and the clean-worktree auto-switch in `start_load`: set
    /// `source`, emit the contract log line, and reload.
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
        self.content_missing = false;
        self.content_undecodable = false;
        self.diff = None;
        // T-WS-EDITOR-002: navigating away always drops the old buffer's
        // dirty/external-changed state — callers that must NOT silently
        // discard an edit (tree click, ↑/↓ step, source switch) go through
        // `request_select` / `step_selection` / `request_switch_source`,
        // which gate this call behind the unsaved-changes modal first.
        self.dirty = false;
        self.external_changed = false;
        klog!("editor-ws: file {}", file.path.display());
        self.load_selected(cx);
        cx.notify();
    }

    /// Tree-row click entry point (T-WS-EDITOR-002 §5): guards a dirty
    /// buffer before switching files. Re-clicking the already-selected file
    /// is not a navigation — it falls through to `select` even while dirty
    /// (existing refresh behaviour, not a discard).
    pub fn request_select(&mut self, file_index: usize, cx: &mut Context<Self>) {
        if should_guard_navigation(self.dirty, self.selected, file_index) {
            self.open_dirty_guard(EditorPendingIntent::SelectFile(file_index), cx);
            return;
        }
        self.select(file_index, cx);
    }

    /// Chip-click entry point for switching the tree source (T-WS-EDITOR-002
    /// §5): guards a dirty buffer the same way as `request_select`. The
    /// clean-worktree auto-switch in `start_load` calls `switch_source`
    /// directly — at that point nothing has loaded yet, so there is nothing
    /// to guard.
    pub fn request_switch_source(&mut self, source: TreeSource, cx: &mut Context<Self>) {
        if self.source == source {
            return;
        }
        if self.dirty {
            self.open_dirty_guard(EditorPendingIntent::SwitchSource(source), cx);
            return;
        }
        self.switch_source(source, cx);
    }

    /// Defer to the parent to open the unsaved-changes confirmation
    /// (`ActiveModal` lives on `KagiApp`, ADR-0076/0093). Deferred via
    /// `cx.spawn` rather than a synchronous `self.app.update` — the arrow-key
    /// path (`KagiApp::step_editor_ws_selection`) calls into this entity from
    /// within an already-leased `KagiApp` listener, so a synchronous call
    /// back into `KagiApp` here would double-lease it and panic (same
    /// hazard as `conflict_view`'s `marshal_error_toast`).
    fn open_dirty_guard(&self, intent: EditorPendingIntent, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.open_editor_dirty_guard(intent, cx);
            });
        })
        .detach();
    }

    /// Load the selected file's raw text (off-thread, guarded by
    /// `MAX_EDITOR_BYTES` + `MAX_EDITOR_LINES` + a binary probe) and its WIP
    /// diff (unstaged, falling back to staged — mirrors
    /// `FileHistoryView::load_diff`).
    ///
    /// T-WS-EDITOR-005 finding #2: the size guard runs BEFORE any read (an
    /// oversized file is never loaded into memory just to be told "too
    /// large"), the binary probe only looks at the leading
    /// `BINARY_PROBE_BYTES` of the raw bytes, and the line count is computed
    /// here in the background task — the marshal-back below only assigns.
    fn load_selected(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.selected else { return };
        let Some(file) = self.files.get(idx) else {
            return;
        };
        let path = file.path.clone();
        // T-WS-EDITOR-005 finding #6: a file git already reports as deleted
        // always gets the "deleted" placeholder, even if a stale read somehow
        // still succeeds (e.g. a race with the working tree).
        let deleted = matches!(file.change, Some(ChangeKind::Deleted));
        self.file_loading = true;
        self.file_req = self.file_req.wrapping_add(1);
        let file_req = self.file_req;
        let generation = self.generation;
        let repo_path = self.repo_path.clone();
        let bg_path = path.clone();

        let task = cx.background_spawn(async move {
            let full_path = repo_path.join(&bg_path);

            let too_big = std::fs::metadata(&full_path)
                .map(|m| m.len() > MAX_EDITOR_BYTES)
                .unwrap_or(false);

            let (text, is_binary, missing, undecodable, too_many_lines) = if too_big {
                (None, false, false, false, false)
            } else {
                match std::fs::read(&full_path) {
                    Ok(bytes) => {
                        // Binary probe on raw bytes, BEFORE any UTF-8
                        // decoding, and only the leading slice.
                        let probe_end = bytes.len().min(BINARY_PROBE_BYTES);
                        let is_binary =
                            kagi_domain::checklist::content_looks_binary(&bytes[..probe_end]);
                        if is_binary {
                            (None, true, false, false, false)
                        } else {
                            match String::from_utf8(bytes) {
                                Ok(t) => {
                                    let too_many_lines = t.lines().count() > MAX_EDITOR_LINES;
                                    if too_many_lines {
                                        (None, false, false, false, true)
                                    } else {
                                        (Some(t), false, false, false, false)
                                    }
                                }
                                Err(_) => (None, false, false, true, false),
                            }
                        }
                    }
                    Err(_) => (None, false, true, false, false),
                }
            };
            let too_large = too_big || too_many_lines;

            let diff = kagi_git::Backend::open(&repo_path).ok().and_then(|repo| {
                match repo.unstaged_file_diff(&bg_path) {
                    Ok(d) if !d.hunks.is_empty() || d.is_binary => Some(d),
                    _ => repo.staged_file_diff(&bg_path).ok(),
                }
            });
            (text, is_binary, missing, undecodable, too_large, diff)
        });

        cx.spawn(async move |view, acx| {
            let (text, is_binary, missing, undecodable, too_large, file_diff) = task.await;
            let _ = view.update(acx, |v, cx| {
                if v.file_req != file_req || v.generation != generation {
                    return;
                }
                v.file_loading = false;
                v.content_binary = is_binary;
                v.content_too_large = too_large;
                v.content_missing = missing || deleted;
                v.content_undecodable = undecodable;
                v.content_lang = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(lang_for_ext)
                    .unwrap_or("text");
                // T-WS-EDITOR-005 finding #9: compute the content signature
                // ONCE here (load time), not every render.
                v.content_sig = text
                    .as_ref()
                    .map(|t| conflict_content_sig(&path, t, false))
                    .unwrap_or(0);
                v.content = text;
                v.diff = file_diff.map(|d| {
                    build_main_diff_view(
                        &d,
                        &path,
                        0,
                        MainDiffSource::Unstaged { path: path.clone() },
                    )
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Lazily create the code-viewer `InputState` (needs a `Window`, only
    /// available in `Render`) and push the selected file's content into it,
    /// guarded by a content-hash sig so a re-render that changed nothing
    /// doesn't clobber the viewer (scroll/selection would reset otherwise).
    ///
    /// T-WS-EDITOR-005 finding #9: `content_sig` is precomputed once at load
    /// time (`load_selected`'s marshal-back) — this only compares it against
    /// `pushed_sig` and clones `content` when it actually needs to push,
    /// instead of cloning + rehashing the whole file on every render.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_none() {
            return;
        }
        if self.editor.is_none() {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor(self.content_lang)
                    .line_number(true)
            });
            // T-WS-EDITOR-002: track user edits for the dirty indicator/
            // guard/save-gate. `InputState` emits `InputEvent::Change` (see
            // gpui-component 0.5.1 `src/input/state.rs::replace_text_in_range`)
            // on every keystroke/paste/cut/undo AND on our own `set_value`
            // push below — see `dirty`'s doc comment for why the handler
            // compares text against `content` instead of a synchronous
            // "am I mid-push" flag. Detached rather than stored: same
            // pattern as the theme-select `Confirm` subscription in
            // `mod.rs`'s `open_main_window` (holds a weak ref internally, so
            // a dropped entity — workspace closed — just makes the callback
            // a no-op).
            cx.subscribe(&state, |this: &mut Self, state, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let current = state.read(cx).value();
                let dirty = this.content.as_deref() != Some(current.as_ref());
                if this.dirty != dirty {
                    this.dirty = dirty;
                    cx.notify();
                }
            })
            .detach();
            self.editor = Some(state);
        }
        if self.content_sig == self.pushed_sig {
            return;
        }
        self.pushed_sig = self.content_sig;
        let Some(content) = self.content.clone() else {
            return;
        };
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
            None => {
                // The selected file might be real but hidden (collapsed
                // away) rather than nonexistent — find its base `tree` index
                // so we can step from where it *would* be instead of
                // teleporting to an end (T-WS-EDITOR-005 finding #7).
                let hidden_base_index = self.selected.and_then(|sel| {
                    self.tree.iter().position(
                        |r| matches!(r, TreeRow::File { file_index, .. } if *file_index == sel),
                    )
                });
                match hidden_base_index {
                    Some(base) => nearest_visible_file_row(&visible, &file_rows, base, delta),
                    // No selection at all yet: ↓ starts at the top, ↑ at the bottom.
                    None if delta >= 0 => 0,
                    None => file_rows.len() - 1,
                }
            }
        };
        let (vis_pos, file_index) = file_rows[next];
        if self.selected == Some(file_index) {
            return;
        }
        // T-WS-EDITOR-002 §5: ↑/↓ stepping to a different file must not
        // silently discard a dirty buffer — checked before the scroll so a
        // cancelled step doesn't leave the tree scrolled to a row that isn't
        // actually selected.
        if should_guard_navigation(self.dirty, self.selected, file_index) {
            self.open_dirty_guard(EditorPendingIntent::SelectFile(file_index), cx);
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

    /// Expand every directory (the ⌄⌄ button in the chip row).
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        cx.notify();
    }

    /// Collapse every directory (the ⌃⌃ button in the chip row).
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed = all_dir_indices(&self.tree);
        cx.notify();
    }

    /// Ask the parent to close this view (drops the entity). Safe per
    /// ADR-0117: only clears fields, never re-leases this entity. Guarded by
    /// the unsaved-changes modal when dirty (T-WS-EDITOR-002 §5) — this is a
    /// plain header-click listener (not nested inside another `KagiApp`
    /// lease), so the direct `self.app.update` call is safe either way, same
    /// as the existing clean-close path below.
    fn request_close(&self, cx: &mut Context<Self>) {
        if self.dirty {
            self.open_dirty_guard(EditorPendingIntent::Close, cx);
            return;
        }
        let _ = self.app.update(cx, |app, cx| {
            app.close_editor_workspace();
            cx.notify();
        });
    }

    /// Cmd-S (T-WS-EDITOR-002 §3): write `editor`'s current full text to
    /// `repo_path.join(path)` on a background thread. No-op if there's
    /// nothing to save (defensive — `KagiApp::save_editor_file` already
    /// checks `dirty` before calling this).
    ///
    /// This is a plain file write, NOT a Git operation — ADR-0120 §4
    /// explicitly scopes saving out of the plan→confirm→execute pipeline
    /// (invariant 4 governs Git writes; `std::fs::write` here is not one).
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        let Some(idx) = self.selected else { return };
        let Some(file) = self.files.get(idx) else {
            return;
        };
        let Some(editor) = self.editor.clone() else {
            return;
        };
        let path = file.path.clone();
        let full_path = editor_save_path(&self.repo_path, &path);
        let text = editor.read(cx).value().to_string();

        let task = cx.background_spawn(async move {
            std::fs::write(&full_path, text.as_bytes()).map_err(|e| e.to_string())
        });
        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| match result {
                Ok(()) => {
                    klog!("editor-ws: saved {}", path.display());
                    v.dirty = false;
                    v.external_changed = false;
                    // Simplest correct refresh (ADR-0120 / ticket): re-run
                    // the existing file-list load, which restores the
                    // selection and — via `select`'s cascade into
                    // `load_selected` — reloads this file's content + diff
                    // too. Both loaders are generation-guarded already.
                    v.start_load(cx);
                    cx.notify();
                }
                Err(e) => {
                    klog!("editor-ws: save failed: {}", e);
                    // Non-git file error: surface via toast + footer (the
                    // established precedent for a background op outside the
                    // plan pipeline — e.g. `repo.fetch`'s failure path,
                    // `commands.rs`), not a git plan modal.
                    let msg = format!("Save failed: {}: {}", path.display(), e);
                    let _ = v.app.update(cx, |app, cx| {
                        app.push_toast(ToastKind::Error, msg.clone(), cx);
                        app.status_footer = FooterStatus::Failed(SharedString::from(msg));
                        cx.notify();
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// FS-watcher nudge (T-WS-EDITOR-002 §4), called from
    /// `KagiApp::refresh_working_tree_external` on every debounced
    /// `WatchEvent::WorkTree`. Always refreshes the tree/badges (also covers
    /// the tree-side of a just-completed save); additionally re-reads the
    /// selected file's content when the buffer is clean, or raises the
    /// "changed on disk" banner when it's dirty (never clobbers an edit).
    pub fn on_worktree_changed(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            self.external_changed = true;
        }
        self.start_load(cx);
        cx.notify();
    }

    /// The dirty-changed banner's Reload button: discard the buffer and
    /// re-read the selected file from disk.
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.dirty = false;
        self.external_changed = false;
        if let Some(idx) = self.selected {
            self.select(idx, cx);
        }
        cx.notify();
    }
}

/// T-WS-EDITOR-002 §3: resolve the on-disk path `save` writes to. Pure —
/// extracted so the join is unit-testable without a `Context` (`repo_path`
/// is absolute, `rel` is repo-relative — same shape `load_selected` already
/// uses for reads).
fn editor_save_path(repo_path: &Path, rel: &Path) -> PathBuf {
    repo_path.join(rel)
}

/// T-WS-EDITOR-002 §5: whether navigating to `target` (tree click / ↑/↓ step)
/// must first open the unsaved-changes confirmation instead of switching
/// immediately. Pure — extracted from `request_select`/`step_selection` so
/// the dirty-guard decision is unit-testable without a `Context`.
fn should_guard_navigation(dirty: bool, selected: Option<usize>, target: usize) -> bool {
    dirty && selected != Some(target)
}

/// T-WS-EDITOR-002 §4: whether `start_load`'s restore-selection step must
/// preserve a dirty buffer's selection index (`candidate`) WITHOUT calling
/// `select` (which would clobber `content`/`diff`/`dirty`) — true only when
/// the buffer is dirty AND `restore` found the SAME file (by path) at
/// `candidate`. Pure — extracted so the "don't clobber an unsaved edit"
/// decision is unit-testable without a `Context`.
fn preserve_dirty_selection(dirty: bool, restore: Option<usize>, candidate: usize) -> bool {
    dirty && restore == Some(candidate)
}

/// Every directory row's index into `tree` — the "fully collapsed" set used
/// by `collapse_all` and as `TreeSource::All`'s initial state.
fn all_dir_indices(tree: &[TreeRow]) -> HashSet<usize> {
    tree.iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, TreeRow::Dir { .. }))
        .map(|(i, _)| i)
        .collect()
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

/// Where a *hidden* selected file (identified by its base index into `tree`)
/// should land among the visible file rows when stepping by `delta`, instead
/// of teleporting to an end (T-WS-EDITOR-005 finding #7).
///
/// `visible` maps a visible position to its base `tree` index; `file_rows` is
/// `(visible_position, file_index)` pairs for the visible file rows only, in
/// tree order. Finds the insertion point — the first visible file row whose
/// base index comes after `hidden_base_index` — then steps one further for
/// `delta < 0` (select the file just before it) or stays put for `delta >= 0`
/// (select the file right after it), each clamped to the array bounds. Pure —
/// unit-tested below.
fn nearest_visible_file_row(
    visible: &[usize],
    file_rows: &[(usize, usize)],
    hidden_base_index: usize,
    delta: i32,
) -> usize {
    let insert_pos = file_rows
        .iter()
        .position(|&(vis_pos, _)| visible[vis_pos] >= hidden_base_index)
        .unwrap_or(file_rows.len());
    if delta >= 0 {
        insert_pos.min(file_rows.len() - 1)
    } else {
        insert_pos.saturating_sub(1)
    }
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

impl Render for EditorWorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_editor(window, cx);
        render_editor_workspace(self, cx)
    }
}

// ── KagiApp entry points (ADR-0117 / ADR-0120) ─────────────────────

impl super::KagiApp {
    /// Open the Editor workspace for the current repo. No-op when no
    /// repository is open.
    ///
    /// T-WS-EDITOR-005 finding #4: close the File History / Ecosystem
    /// takeovers first. Both beat Editor mode in `resolve_workspace`, so
    /// leaving one open would mean Cmd-Shift-E silently does nothing visible
    /// — the entity would open in the background but the resolver would keep
    /// showing the takeover. The reverse direction (opening Analyze over an
    /// open Editor) is left as-is: that's the existing, intended overlay
    /// behavior (the editor entity keeps running, just hidden).
    pub fn open_editor_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        self.close_file_history();
        self.close_ecosystem_view();
        let weak = cx.weak_entity();
        let view = cx.new(|_| EditorWorkspaceView::new(weak, repo_path));
        self.editor_workspace = Some(view.clone());
        klog!("editor-ws: open");
        view.update(cx, |v, cx| v.start_load(cx));
        cx.notify();
    }

    /// Close the Editor workspace (drops the entity; Graph mode is derived
    /// from `editor_workspace.is_none()`).
    pub fn close_editor_workspace(&mut self) {
        self.editor_workspace = None;
    }

    /// Root ↑/↓ handler branch for the Editor workspace: step the file
    /// selection on the entity (normal parent→child `update`; the entity is
    /// not leased here).
    pub fn step_editor_ws_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(ev) = self.editor_workspace.clone() {
            ev.update(cx, |v, cx| v.step_selection(delta, cx));
        }
    }

    /// Cmd-S root handler (T-WS-EDITOR-002 §3): save the Editor Workspace's
    /// buffer if it's open and dirty; no-op otherwise (closed workspace,
    /// clean buffer, or the "Save" keystroke firing on the wrong tab).
    pub fn save_editor_file(&mut self, cx: &mut Context<Self>) {
        let Some(ev) = self.editor_workspace.clone() else {
            return;
        };
        if !ev.read(cx).dirty {
            return;
        }
        ev.update(cx, |v, cx| v.save(cx));
    }

    /// Open the unsaved-changes confirmation before discarding the Editor
    /// Workspace's dirty buffer (T-WS-EDITOR-002 §5). `ActiveModal` lives on
    /// `KagiApp` per the CLAUDE.md state-update rules; `intent` is what to do
    /// once the user confirms Discard.
    pub fn open_editor_dirty_guard(&mut self, intent: EditorPendingIntent, cx: &mut Context<Self>) {
        self.set_editor_dirty_guard_modal(EditorDirtyGuardModal { intent });
        klog!("editor-ws: dirty-guard");
        cx.notify();
    }

    /// Esc / Cancel on the unsaved-changes modal: dismiss without acting.
    pub fn cancel_editor_dirty_guard(&mut self) {
        self.clear_editor_dirty_guard_modal();
    }

    /// Enter / Discard on the unsaved-changes modal: drop the confirmation
    /// and run the pending action. The entity's `select`/`switch_source`
    /// reset `dirty` as part of navigating away, so no separate "discard"
    /// step is needed here.
    pub fn confirm_editor_dirty_guard(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_dirty_guard_modal().cloned() else {
            return;
        };
        self.clear_editor_dirty_guard_modal();
        match modal.intent {
            EditorPendingIntent::SelectFile(idx) => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.select(idx, cx));
                }
            }
            EditorPendingIntent::SwitchSource(source) => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.switch_source(source, cx));
                }
            }
            EditorPendingIntent::Close => {
                self.close_editor_workspace();
            }
        }
        cx.notify();
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
    // T-WS-EDITOR-002 §2: "● " prefix while the open buffer has unsaved edits.
    let selected_path = view.selected.and_then(|i| view.files.get(i)).map(|f| {
        let path = f.path.to_string_lossy();
        if view.dirty {
            SharedString::from(format!("\u{25cf} {}", path))
        } else {
            SharedString::from(path.into_owned())
        }
    });

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

    // T-WS-EDITOR-005 finding #3: `show_tree` (pushed by `render_body` from
    // the resolver's `layout.left`) hides the tree pane AND its divider —
    // View → Toggle Sidebar now actually hides the file tree in Editor mode.
    let body = div()
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .when(view.show_tree, |el| {
            el.child(render_tree_pane(view, cx)).child(tree_divider)
        })
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
        this.request_switch_source(source, cx);
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

/// A small fold/unfold icon button in the chip row (expand-all /
/// collapse-all — user request).
fn render_fold_button(
    id: &'static str,
    icon_path: &'static str,
    on_click: impl Fn(&mut EditorWorkspaceView, &mut Context<EditorWorkspaceView>) + 'static,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        on_click(this, cx);
    });
    div()
        .id(id)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .w(theme::scaled_px(20.0))
        .h(theme::scaled_px(18.0))
        .rounded_sm()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .child(
            gpui_component::Icon::default()
                .path(icon_path)
                .with_size(gpui_component::Size::Size(theme::scaled_px(13.0)))
                .text_color(rgb(theme().text_muted)),
        )
        .into_any_element()
}

/// The Changes/All tree-source toggle row + expand/collapse-all buttons,
/// pinned above the tree list (T-WS-EDITOR-004 user feedback #1).
fn render_source_chips(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    div()
        .id("ews-source-chips")
        .flex()
        .flex_row()
        .items_center()
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
        // Expand-all (⌄⌄) / collapse-all (⌃⌃), lucide unfold/fold glyphs.
        .child(render_fold_button(
            "ews-expand-all",
            "icons/chevrons-up-down.svg",
            |this, cx| this.expand_all(cx),
            cx,
        ))
        .child(render_fold_button(
            "ews-collapse-all",
            "icons/chevrons-down-up.svg",
            |this, cx| this.collapse_all(cx),
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
                // `i` is the visible/uniform_list position — passed through
                // for zebra striping (T-WS-EDITOR-005 finding #8), which must
                // key on visible position, not the base `tree` index.
                range
                    .filter_map(|i| render_tree_row(this, i, *visible.get(i)?, cx))
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
/// `visible_pos` is the row's position in the uniform_list's visible range
/// (used for zebra striping — T-WS-EDITOR-005 finding #8); `index` is its base
/// index into `view.tree`.
fn render_tree_row(
    view: &EditorWorkspaceView,
    visible_pos: usize,
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
            // T-WS-EDITOR-005 finding #8: stripe by visible position, not the
            // base tree index — collapsing a directory must not leave the
            // remaining visible rows with a broken (skip-a-beat) checkerboard.
            let row_bg = if is_selected {
                theme().selected
            } else if visible_pos % 2 == 1 {
                theme().bg_row_alt
            } else {
                theme().panel
            };
            // T-WS-EDITOR-002 §2: only the selected row can be the open
            // (possibly dirty) buffer — v1 has a single editable buffer.
            let show_dirty_dot = is_selected && view.dirty;
            let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.request_select(file_index, cx);
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
                    .when(show_dirty_dot, |el| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .pl(theme::scaled_px(4.))
                                .text_xs()
                                .text_color(rgb(theme().color_warning))
                                .child(SharedString::from("\u{25cf}")),
                        )
                    })
                    .into_any_element(),
            )
        }
    }
}

/// Center pane: the selected file's editable code viewer, or a placeholder.
fn render_center_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> gpui::AnyElement {
    let mut pane = div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_base));

    // T-WS-EDITOR-002 §4: external-change banner — only when the buffer is
    // dirty (a clean buffer just silently re-reads via `on_worktree_changed`).
    if view.external_changed {
        let reload = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.reload_from_disk(cx);
        });
        pane = pane.child(
            div()
                .id("ews-external-changed")
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .w_full()
                .gap_2()
                .px_3()
                .py_1()
                .bg(rgb(theme().color_warning))
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(rgb(theme().bg_base))
                        .child(Msg::EditorWorkspaceExternalChanged.t()),
                )
                .child(
                    div()
                        .id("ews-reload")
                        .px_2()
                        .py_px()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .bg(rgb(theme().bg_base))
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)))
                        .on_click(reload)
                        .child(Msg::EditorWorkspaceReload.t()),
                ),
        );
    }

    // T-WS-EDITOR-005 finding #6: deleted/missing and non-UTF-8 files used to
    // both fall through to the generic "select a file" placeholder even
    // though a row was actually selected — distinguish why `content` is
    // `None` instead.
    let placeholder = if view.loading {
        Some(Msg::EditorWorkspaceLoading.t())
    } else if view.selected.is_none() {
        Some(Msg::EditorWorkspaceSelectFile.t())
    } else if view.file_loading {
        Some(Msg::EditorWorkspaceLoading.t())
    } else if view.content_missing {
        Some(Msg::EditorWorkspaceDeleted.t())
    } else if view.content_binary {
        Some(Msg::EditorWorkspaceBinary.t())
    } else if view.content_too_large {
        Some(Msg::EditorWorkspaceTooLarge.t())
    } else if view.content_undecodable {
        Some(Msg::EditorWorkspaceUndecodable.t())
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
                // T-WS-EDITOR-002: editable now — `disabled(true)` gated
                // typing/paste/cut/undo behind gpui-component 0.5.1's
                // `!disabled` check AND kept a disabled input from taking
                // focus at all, which is why ↑/↓ fell through to the root
                // file-stepping handlers instead of moving the cursor
                // (user-reported). `appearance(false)` / `bordered(false)`
                // are kept so it still doesn't look like a form field.
                Input::new(&editor)
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
    fn all_dir_indices_collects_every_dir() {
        let tree = sample_tree();
        // sample_tree: dirs at 0 (src) and 2 (ui).
        let dirs = all_dir_indices(&tree);
        assert_eq!(dirs, [0, 2].into_iter().collect());
        // Fully collapsed: only top-level rows remain visible.
        assert_eq!(visible_tree_indices(&tree, &dirs), vec![0, 4]);
    }

    #[test]
    fn collapsed_state_of_a_hidden_dir_is_inert() {
        let tree = sample_tree();
        // src collapsed AND the hidden ui collapsed — same as src alone.
        let collapsed: HashSet<usize> = [0, 2].into_iter().collect();
        assert_eq!(visible_tree_indices(&tree, &collapsed), vec![0, 4]);
    }

    // ── T-WS-EDITOR-005 finding #7: nearest_visible_file_row ───────────────

    #[test]
    fn nearest_visible_file_row_steps_from_hidden_selection() {
        // sample_tree() with "ui" (base idx 2) collapsed: visible = [0,1,2,4],
        // hiding b.rs (file_index 1, base idx 3). Visible files: a.rs
        // (file_index 0) at visible pos 1, root.rs (file_index 2) at visible
        // pos 3.
        let visible = vec![0usize, 1, 2, 4];
        let file_rows = vec![(1usize, 0usize), (3usize, 2usize)];
        let hidden_base_index = 3; // b.rs's base tree index

        // Down: the next visible file after the hidden one (root.rs, pos 1
        // in `file_rows`).
        assert_eq!(
            nearest_visible_file_row(&visible, &file_rows, hidden_base_index, 1),
            1
        );
        // Up: the visible file before the hidden one (a.rs, pos 0).
        assert_eq!(
            nearest_visible_file_row(&visible, &file_rows, hidden_base_index, -1),
            0
        );
    }

    #[test]
    fn nearest_visible_file_row_clamps_at_ends() {
        // "src" collapsed: only root.rs (file_index 2) is a visible file, at
        // visible pos 1.
        let visible = vec![0usize, 4];
        let file_rows = vec![(1usize, 2usize)];

        // Hidden a.rs (base idx 1, before every visible file): up clamps to
        // the first (only) visible file rather than underflowing.
        assert_eq!(nearest_visible_file_row(&visible, &file_rows, 1, -1), 0);
        assert_eq!(nearest_visible_file_row(&visible, &file_rows, 1, 1), 0);

        // A hidden base index after every visible file: down clamps to the
        // last visible file instead of overflowing.
        assert_eq!(nearest_visible_file_row(&visible, &file_rows, 100, 1), 0);
        assert_eq!(nearest_visible_file_row(&visible, &file_rows, 100, -1), 0);
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

    // ── T-WS-EDITOR-002: editable-buffer pure logic ────────────────────────

    #[test]
    fn editor_save_path_joins_repo_root_and_relative_path() {
        let repo = Path::new("/repo");
        let rel = Path::new("src/main.rs");
        assert_eq!(
            editor_save_path(repo, rel),
            PathBuf::from("/repo/src/main.rs")
        );
    }

    #[test]
    fn should_guard_navigation_only_when_dirty_and_actually_switching() {
        // Clean buffer: never guard, regardless of target.
        assert!(!should_guard_navigation(false, Some(0), 1));
        assert!(!should_guard_navigation(false, None, 0));
        // Dirty, but re-clicking/re-stepping to the SAME file: not a
        // discard, so don't guard (existing refresh behaviour).
        assert!(!should_guard_navigation(true, Some(2), 2));
        // Dirty and actually switching to a different file: guard.
        assert!(should_guard_navigation(true, Some(2), 3));
        assert!(should_guard_navigation(true, None, 0));
    }

    #[test]
    fn preserve_dirty_selection_only_when_dirty_and_restore_matches_candidate() {
        // Clean buffer: never preserve — the normal `select` cascade runs.
        assert!(!preserve_dirty_selection(false, Some(1), 1));
        // Dirty, but `restore` didn't find this file (e.g. fell back to the
        // first file in the list, a different one) — don't just leave the
        // index dangling on a mismatch.
        assert!(!preserve_dirty_selection(true, None, 0));
        assert!(!preserve_dirty_selection(true, Some(1), 0));
        // Dirty AND `restore` found the same file (by path) at `candidate` —
        // preserve the buffer, just remap the index.
        assert!(preserve_dirty_selection(true, Some(4), 4));
    }
}
