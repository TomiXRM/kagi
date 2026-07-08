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
#[derive(Clone, Debug, Default)]
pub struct WorkspaceFile {
    pub path: PathBuf,
    pub change: Option<ChangeKind>,
    /// `true` when this path has no git history at all (T-WS-EDITOR-007) —
    /// gates "Add to .gitignore" (untracked only) and "Discard Changes…"
    /// (tracked only) in the tree context menu.
    pub untracked: bool,
    /// `true` when this path has unstaged (working-tree) changes —
    /// gates "Stage" in the tree context menu.
    pub unstaged: bool,
    /// `true` when this path has staged (index) changes — gates "Unstage".
    pub staged: bool,
}

/// Where a tree right-click landed (T-WS-EDITOR-007): a file row, a directory
/// row, or the empty area below the tree (repo root). Indices key into the
/// owning `EditorWorkspaceView`'s `files` (`File`) / `tree` (`Dir`) — resolved
/// to a concrete path/flags when the menu overlay is built (render time),
/// same latency window as every other right-click menu in this codebase
/// (commit/branch/stash/conflict-file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeMenuTarget {
    /// Index into `files`.
    File(usize),
    /// Index into `tree` (a `TreeRow::Dir`).
    Dir(usize),
    /// The empty area below the tree, or repo root for New File/New Folder.
    Root,
}

/// One open-but-INACTIVE editor tab's buffer (user request: editor tabs).
/// The same per-file fields `EditorWorkspaceView` holds flattened for the
/// active buffer — swapped wholesale on tab switch (`stash_active` /
/// `open_tab`), mirroring `KagiApp`'s `active_view`/`tab_cache` pattern
/// (ADR-0075). Each tab keeps its OWN `InputState`: sharing one and
/// `set_value`-swapping text would leak the undo history across files
/// (gpui-component's `set_value` ignores history, so Cmd-Z in tab B would
/// replay tab A's edits).
struct EditorBufferState {
    content: Option<String>,
    content_lang: &'static str,
    content_binary: bool,
    content_too_large: bool,
    content_missing: bool,
    content_undecodable: bool,
    content_sig: u64,
    editor: Option<Entity<InputState>>,
    pushed_sig: u64,
    dirty: bool,
    external_changed: bool,
    diff: Option<MainDiffView>,
}

/// Result of the background save write (`save_impl`): `Conflict` means the
/// on-disk bytes no longer match the buffer's loaded snapshot, so nothing was
/// written — the user decides via the external-change banner.
enum SaveOutcome {
    Saved(String),
    Conflict,
    Failed(String),
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

    /// Index into `files` of the highlighted tree row, if any. Purely a tree
    /// highlight — the open buffer is identified by `open_path`
    /// (T-WS-EDITOR-002 spec change: the tree source is a view filter, so
    /// the open file may legitimately be absent from `files`, e.g. a clean
    /// open file after switching All → Changes; then `selected` is `None`
    /// while the buffer stays open).
    pub selected: Option<usize>,
    /// Repo-relative path of the file the ACTIVE buffer holds (set by
    /// `open_tab`). The header path, the content/diff loader, and save all
    /// key off this — NOT off `selected` — so a tree-source switch can
    /// rebuild the list without touching the open buffer.
    pub open_path: Option<PathBuf>,
    /// Editor tab order (user request: multiple open buffers). Contains the
    /// active tab too; the ACTIVE buffer's fields stay flattened on this
    /// struct, inactive buffers live in `tab_cache` — the same
    /// active+cache swap pattern as `KagiApp.active_view`/`tab_cache`
    /// (ADR-0075), so the render/loader code keeps reading the flat fields.
    pub open_tabs: Vec<PathBuf>,
    /// Stashed per-file state for the INACTIVE tabs, keyed by path.
    tab_cache: HashMap<PathBuf, EditorBufferState>,
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
    /// T-DIFF-WRAP-001: `ListState` (variable-height) for the right hunks
    /// list — see `render_helpers::render_diff_list` for the item-count
    /// sync/reset lifecycle.
    pub diff_scroll: gpui::ListState,

    /// Whether the left tree pane renders (T-WS-EDITOR-005 finding #3).
    /// Pushed by `render_body` from `workspace::resolve_workspace`'s
    /// `layout.left` right before this entity is embedded — the resolver's
    /// output stays the single source of truth for View → Toggle Sidebar even
    /// though this entity self-renders and can't be skipped slot-by-slot the
    /// way the sidebar/right-panel arms are. Defaults to `true` so the tree
    /// shows before the first `render_body` push.
    pub show_tree: bool,

    /// The tree's right-click context menu (T-WS-EDITOR-007): which row (or
    /// the empty area) it targets, and the click position for the overlay.
    /// Rendered TOP-LEVEL on `KagiApp` (mirrors `ConflictView::file_menu` —
    /// see `editor_tree_menu::render_editor_tree_menu` + `render.rs`'s
    /// `editor_tree_menu_overlay`), never inside this entity's `Render` —
    /// its actions dispatch on `KagiApp` directly (fs mutations, modals),
    /// never re-entering this leased entity.
    pub tree_menu: Option<(TreeMenuTarget, gpui::Point<gpui::Pixels>)>,
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
            open_path: None,
            open_tabs: Vec::new(),
            tab_cache: HashMap::new(),
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
            diff_scroll: new_diff_list_state(),
            show_tree: true,
            tree_menu: None,
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

                        // T-WS-EDITOR-002 spec change (user): a tree reload
                        // (source switch / watcher / post-save) is a VIEW
                        // update — it must never touch the open buffer.
                        // With a buffer open, only remap the highlight to
                        // the open file's new row (or clear it if the file
                        // isn't listed — the buffer stays open, identified
                        // by the header path). `select` (which reloads
                        // content and resets `dirty`) runs only on the very
                        // first load, when nothing is open yet.
                        let restore = v
                            .open_path
                            .as_ref()
                            .and_then(|p| v.files.iter().position(|f| &f.path == p));
                        let (sel, load) =
                            restore_selection(v.open_path.is_some(), restore, v.files.len());
                        match (sel, load) {
                            (Some(i), true) => v.select(i, cx),
                            (sel, _) => v.selected = sel,
                        }
                    }
                    Err(e) => v.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shared by `set_source` (the manual chip toggle) and the
    /// clean-worktree auto-switch in `start_load`: set `source`, emit the
    /// contract log line, and reload.
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

    /// Select a tree row's file (index into `files`): make it the active
    /// tab. NOT a destructive action anymore (editor tabs, user request) —
    /// a dirty buffer is stashed into its tab, never discarded, so no
    /// dirty guard is needed on file switch.
    pub fn select(&mut self, file_index: usize, cx: &mut Context<Self>) {
        let Some(file) = self.files.get(file_index) else {
            return;
        };
        self.selected = Some(file_index);
        self.open_tab(file.path.clone(), cx);
    }

    /// Make `path` the active tab: stash (or replace) the current active
    /// buffer, then restore `path`'s cached buffer or load it fresh.
    ///
    /// ponytail: a CLEAN active tab is replaced (not stashed) when opening
    /// a file that isn't already a tab — otherwise ↑/↓ browsing through the
    /// tree would spam one tab per step. Dirty tabs always survive. Upgrade
    /// path if users want sticky clean tabs: a pinned/preview distinction
    /// (VSCode-style), tracked per tab.
    pub fn open_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.open_path.as_ref() == Some(&path) {
            return;
        }
        let is_new = !self.open_tabs.contains(&path);
        if let Some(active) = self.open_path.clone() {
            if is_new && !self.dirty {
                self.open_tabs.retain(|p| p != &active);
                self.open_path = None;
                self.reset_active_buffer();
            } else {
                self.stash_active();
            }
        }
        if is_new {
            self.open_tabs.push(path.clone());
        }
        klog!("editor-ws: file {}", path.display());
        if let Some(buf) = self.tab_cache.remove(&path) {
            let clean = !buf.dirty;
            self.content = buf.content;
            self.content_lang = buf.content_lang;
            self.content_binary = buf.content_binary;
            self.content_too_large = buf.content_too_large;
            self.content_missing = buf.content_missing;
            self.content_undecodable = buf.content_undecodable;
            self.content_sig = buf.content_sig;
            self.editor = buf.editor;
            self.pushed_sig = buf.pushed_sig;
            self.dirty = buf.dirty;
            self.external_changed = buf.external_changed;
            self.diff = buf.diff;
            self.open_path = Some(path);
            if clean {
                // Refresh a clean buffer on activation: covers external
                // changes while it was backgrounded AND a load that was
                // dropped mid-flight by a tab switch. Sig-guarded push —
                // an unchanged file is a no-op for the editor.
                self.load_selected(cx);
            }
        } else {
            self.reset_active_buffer();
            self.open_path = Some(path);
            self.load_selected(cx);
        }
        cx.notify();
    }

    /// Tab-strip click: activate an already-open tab (also remaps the tree
    /// highlight, which may be `None` if the file isn't in the current
    /// source's list).
    pub fn activate_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected = self.files.iter().position(|f| f.path == path);
        self.open_tab(path, cx);
    }

    /// Tab ×-button click: close the tab, with the unsaved-changes guard
    /// when its buffer is dirty (user request: closing a dirty tab must ask).
    pub fn request_close_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.tab_dirty(&path) {
            self.open_dirty_guard(EditorPendingIntent::CloseTab(path), cx);
            return;
        }
        self.close_tab_now(&path, cx);
    }

    /// Close a tab unconditionally (clean tab, or dirty after the guard's
    /// Discard). Closing the active tab activates its neighbor (next,
    /// falling back to previous — `next_active_tab`), or empties the pane.
    pub fn close_tab_now(&mut self, path: &Path, cx: &mut Context<Self>) {
        let next = next_active_tab(&self.open_tabs, path, self.open_path.as_deref());
        self.open_tabs.retain(|p| p != path);
        self.tab_cache.remove(path);
        if self.open_path.as_deref() == Some(path) {
            self.open_path = None;
            self.reset_active_buffer();
            match next {
                Some(next) => self.activate_tab(next, cx),
                None => self.selected = None,
            }
        }
        cx.notify();
    }

    /// Move the ACTIVE buffer's fields into `tab_cache` (tab switch).
    fn stash_active(&mut self) {
        let Some(path) = self.open_path.take() else {
            return;
        };
        let buf = EditorBufferState {
            content: self.content.take(),
            content_lang: self.content_lang,
            content_binary: self.content_binary,
            content_too_large: self.content_too_large,
            content_missing: self.content_missing,
            content_undecodable: self.content_undecodable,
            content_sig: self.content_sig,
            editor: self.editor.take(),
            pushed_sig: self.pushed_sig,
            dirty: self.dirty,
            external_changed: self.external_changed,
            diff: self.diff.take(),
        };
        self.tab_cache.insert(path, buf);
        self.reset_active_buffer();
    }

    /// Reset every ACTIVE-buffer field to its empty state (fresh open /
    /// close of the active tab). `open_path`/`selected` are the caller's job.
    fn reset_active_buffer(&mut self) {
        self.content = None;
        self.content_lang = "text";
        self.content_binary = false;
        self.content_too_large = false;
        self.content_missing = false;
        self.content_undecodable = false;
        self.content_sig = 0;
        self.editor = None;
        self.pushed_sig = 0;
        self.dirty = false;
        self.external_changed = false;
        self.file_loading = false;
        self.diff = None;
    }

    /// Whether the tab holding `path` (active or cached) has unsaved edits.
    pub fn tab_dirty(&self, path: &Path) -> bool {
        if self.open_path.as_deref() == Some(path) {
            self.dirty
        } else {
            self.tab_cache.get(path).is_some_and(|b| b.dirty)
        }
    }

    /// Whether ANY open tab has unsaved edits — the workspace-close guard
    /// must cover backgrounded tabs too.
    pub fn any_dirty(&self) -> bool {
        self.dirty || self.tab_cache.values().any(|b| b.dirty)
    }

    /// Whether any dirty open tab would be closed by deleting `path`.
    pub fn any_dirty_under(&self, path: &Path) -> bool {
        self.open_path
            .as_deref()
            .is_some_and(|p| self.dirty && path_is_at_or_under(p, path))
            || self
                .tab_cache
                .iter()
                .any(|(p, b)| b.dirty && path_is_at_or_under(p, path))
    }

    /// Chip-click entry point for switching the tree source. T-WS-EDITOR-002
    /// spec change (user): this is a VIEW filter, not a navigation — it never
    /// touches the open buffer and never opens the dirty guard. `start_load`'s
    /// marshal-back re-highlights the open file if listed, or just clears the
    /// highlight (buffer stays open, header path still identifies it).
    pub fn set_source(&mut self, source: TreeSource, cx: &mut Context<Self>) {
        if self.source == source {
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
        // Keyed off `open_path`, not the tree highlight (T-WS-EDITOR-002 spec
        // change) — the open file may not be listed in the current source.
        let Some(path) = self.open_path.clone() else {
            return;
        };
        // T-WS-EDITOR-005 finding #6: a file git already reports as deleted
        // always gets the "deleted" placeholder, even if a stale read somehow
        // still succeeds (e.g. a race with the working tree).
        let deleted = self
            .files
            .iter()
            .find(|f| f.path == path)
            .is_some_and(|f| matches!(f.change, Some(ChangeKind::Deleted)));
        self.file_loading = true;
        self.file_req = self.file_req.wrapping_add(1);
        let file_req = self.file_req;
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
                // `file_req` alone guards staleness — the buffer is decoupled
                // from the tree generation (T-WS-EDITOR-002 spec change: a
                // source switch mid-load must not discard the content read).
                // The `dirty` check closes a race: if the user typed while a
                // watcher-driven clean re-read was in flight, dropping the
                // result is the only option that doesn't clobber the edit.
                // The `open_path` check keeps a load started for one tab from
                // landing in another after a tab switch (editor tabs) — the
                // switched-to tab re-triggers its own load, so nothing stalls.
                if v.file_req != file_req
                    || v.dirty
                    || v.open_path.as_deref() != Some(path.as_path())
                {
                    return;
                }
                v.file_loading = false;
                v.content_binary = is_binary;
                v.content_too_large = too_large;
                v.content_missing = missing || deleted;
                v.content_undecodable = undecodable;
                v.content_lang = lang_for_path(&path).unwrap_or("text");
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
                if this.editor.as_ref() == Some(&state) {
                    let dirty = this.content.as_deref() != Some(current.as_ref());
                    if this.dirty != dirty {
                        this.dirty = dirty;
                        cx.notify();
                    }
                } else if let Some(buf) = this
                    .tab_cache
                    .values_mut()
                    .find(|b| b.editor.as_ref() == Some(&state))
                {
                    // A deferred Change event can land after its tab was
                    // stashed (editor tabs) — book the dirty bit on the
                    // right buffer instead of the active one.
                    let dirty = buf.content.as_deref() != Some(current.as_ref());
                    if buf.dirty != dirty {
                        buf.dirty = dirty;
                        cx.notify();
                    }
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
        // No dirty guard here (editor tabs, user request): stepping opens
        // the file as the active tab; a dirty buffer is stashed into its
        // tab, never discarded.
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
    /// the unsaved-changes modal when ANY tab is dirty (T-WS-EDITOR-002 §5;
    /// backgrounded tabs count — dropping the entity loses them all) — this
    /// is a plain header-click listener (not nested inside another `KagiApp`
    /// lease), so the direct `self.app.update` call is safe either way, same
    /// as the existing clean-close path below.
    fn request_close(&self, cx: &mut Context<Self>) {
        if self.any_dirty() {
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
        self.save_impl(false, cx);
    }

    /// The external-change banner's Overwrite button: save even though the
    /// file changed on disk (the user explicitly chose to clobber it).
    pub fn save_overwrite(&mut self, cx: &mut Context<Self>) {
        self.save_impl(true, cx);
    }

    fn save_impl(&mut self, force: bool, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        // Keyed off `open_path`, not the tree highlight — the open file may
        // not be listed in the current source (T-WS-EDITOR-002 spec change).
        let Some(path) = self.open_path.clone() else {
            return;
        };
        let Some(editor) = self.editor.clone() else {
            return;
        };
        // Banner already up (watcher flagged an external change): don't
        // write — the banner's Overwrite/Reload buttons are the decision.
        if !force && self.external_changed {
            self.notify_save_blocked(&path, cx);
            return;
        }
        let full_path = editor_save_path(&self.repo_path, &path);
        let text = editor.read(cx).value().to_string();
        // Snapshot the buffer was loaded from (or last saved as): compared
        // against the on-disk bytes at write time to close the watcher's
        // debounce race (~500ms) where an external change hasn't flagged
        // `external_changed` yet.
        let snapshot = if force { None } else { self.content.clone() };

        let task = cx.background_spawn(async move {
            // A missing file is NOT a conflict: saving simply recreates it.
            // Any other read error means the disk-vs-snapshot comparison
            // couldn't run, so fail the save instead of risking a clobber.
            if let Some(snap) = &snapshot {
                match std::fs::read(&full_path) {
                    Ok(disk) => {
                        if disk != snap.as_bytes() {
                            return SaveOutcome::Conflict;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return SaveOutcome::Failed(e.to_string()),
                }
            }
            match std::fs::write(&full_path, text.as_bytes()) {
                Ok(()) => SaveOutcome::Saved(text),
                Err(e) => SaveOutcome::Failed(e.to_string()),
            }
        });
        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| match result {
                SaveOutcome::Saved(text) => {
                    klog!("editor-ws: saved {}", path.display());
                    v.dirty = false;
                    v.external_changed = false;
                    // Adopt the saved text as the buffer's snapshot NOW —
                    // and mark it as already pushed (the editor holds this
                    // exact text), so the disk re-read below computes the
                    // same sig and never `set_value`s (which would reset
                    // the cursor/scroll on every save).
                    let sig = conflict_content_sig(&path, &text, false);
                    v.content = Some(text);
                    v.content_sig = sig;
                    v.pushed_sig = sig;
                    // Refresh the tree badges and the right-pane diff
                    // (T-WS-EDITOR-002 spec change: the tree reload is
                    // highlight-only, so the diff refresh runs explicitly).
                    v.start_load(cx);
                    v.load_selected(cx);
                    cx.notify();
                }
                SaveOutcome::Conflict => {
                    // Raise the banner ourselves — the watcher's debounced
                    // event may still be in flight.
                    v.external_changed = true;
                    v.notify_save_blocked(&path, cx);
                }
                SaveOutcome::Failed(e) => {
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

    /// A save was refused because the file changed on disk: log, toast, and
    /// leave the decision to the external-change banner (Overwrite / Reload).
    fn notify_save_blocked(&mut self, path: &Path, cx: &mut Context<Self>) {
        klog!(
            "editor-ws: save blocked (changed on disk) {}",
            path.display()
        );
        let _ = self.app.update(cx, |app, cx| {
            app.push_toast(ToastKind::Error, Msg::EditorWorkspaceSaveBlocked.t(), cx);
            cx.notify();
        });
        cx.notify();
    }

    /// FS-watcher nudge (T-WS-EDITOR-002 §4), called from
    /// `KagiApp::refresh_working_tree_external` on every debounced
    /// `WatchEvent::WorkTree`. Always refreshes the tree/badges (also covers
    /// the tree-side of a just-completed save); additionally re-reads the
    /// open file's content when the buffer is clean (the tree reload itself
    /// is highlight-only per the spec change), or raises the "changed on
    /// disk" banner when it's dirty (never clobbers an edit).
    pub fn on_worktree_changed(&mut self, cx: &mut Context<Self>) {
        self.start_load(cx);
        if self.dirty {
            self.external_changed = true;
        } else {
            // Clean buffer: re-read content + diff. The content-sig guard in
            // `sync_editor` makes an unchanged re-read a no-op push, so the
            // cursor/scroll survive routine watcher ticks.
            self.load_selected(cx);
        }
        // Backgrounded tabs: a dirty one gets the banner when reactivated
        // (same watcher coarseness as the active buffer — we don't know
        // WHICH file changed); a clean one re-reads on activation anyway
        // (`open_tab`'s clean-refresh), so nothing to do for it here.
        for buf in self.tab_cache.values_mut() {
            if buf.dirty {
                buf.external_changed = true;
            }
        }
        cx.notify();
    }

    /// Discard the buffer and re-read the open file from disk (the confirmed
    /// outcome of the external-change banner's Reload button — the button
    /// itself opens the dirty guard first, T-WS-EDITOR-002 §5 spec change).
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.dirty = false;
        self.external_changed = false;
        // Force the editor push even when the disk text hashes back to the
        // pre-edit snapshot (the user's edit is being discarded either way).
        self.pushed_sig = 0;
        self.load_selected(cx);
        cx.notify();
    }

    // ── Tree context menu + fs-mutation follow-up (T-WS-EDITOR-007) ──

    /// Open the tree's right-click context menu at `pos` for `target`. Set by
    /// `on_mouse_down(MouseButton::Right)` on file rows, dir rows, and the
    /// pane background (`Root`); rendered top-level on `KagiApp` (see module
    /// doc on `tree_menu`).
    pub fn open_tree_menu(
        &mut self,
        target: TreeMenuTarget,
        pos: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tree_menu = Some((target, pos));
        cx.notify();
    }

    /// Dismiss the tree context menu without acting.
    pub fn close_tree_menu(&mut self, cx: &mut Context<Self>) {
        self.tree_menu = None;
        cx.notify();
    }

    /// Remap every reference to `old` (or, for a directory rename, every path
    /// nested under it) to `new` — `open_path`, `open_tabs`, and `tab_cache`
    /// keys. Called by `KagiApp::confirm_editor_fs_prompt` after a successful
    /// `std::fs::rename` so the currently-open buffer (and any backgrounded
    /// tab) keeps pointing at the right file instead of a now-stale path.
    pub fn remap_renamed_path(&mut self, old: &Path, new: &Path, cx: &mut Context<Self>) {
        let remap = |p: &Path| -> Option<PathBuf> {
            if p == old {
                Some(new.to_path_buf())
            } else {
                p.strip_prefix(old).ok().map(|suffix| new.join(suffix))
            }
        };
        if let Some(cur) = self.open_path.clone() {
            if let Some(mapped) = remap(&cur) {
                self.open_path = Some(mapped);
            }
        }
        for p in self.open_tabs.iter_mut() {
            if let Some(mapped) = remap(p) {
                *p = mapped;
            }
        }
        let stale_keys: Vec<PathBuf> = self
            .tab_cache
            .keys()
            .filter(|k| remap(k).is_some())
            .cloned()
            .collect();
        for k in stale_keys {
            if let (Some(mapped), Some(buf)) = (remap(&k), self.tab_cache.remove(&k)) {
                self.tab_cache.insert(mapped, buf);
            }
        }
        cx.notify();
    }

    /// Close every open tab at or under `path` (a file delete closes exactly
    /// that tab; a directory delete closes every tab nested under it) —
    /// called by `KagiApp::confirm_editor_delete` after the fs-level trash
    /// move succeeds. Reuses `close_tab_now`'s existing next-tab-activation
    /// logic per victim, so this is safe even when the active tab is among
    /// the victims.
    pub fn close_paths_under(&mut self, path: &Path, cx: &mut Context<Self>) {
        let victims: Vec<PathBuf> = self
            .open_tabs
            .iter()
            .filter(|p| path_is_at_or_under(p, path))
            .cloned()
            .collect();
        for v in victims {
            self.close_tab_now(&v, cx);
        }
    }
}

/// T-WS-EDITOR-002 §3: resolve the on-disk path `save` writes to. Pure —
/// extracted so the join is unit-testable without a `Context` (`repo_path`
/// is absolute, `rel` is repo-relative — same shape `load_selected` already
/// uses for reads).
fn editor_save_path(repo_path: &Path, rel: &Path) -> PathBuf {
    repo_path.join(rel)
}

fn path_is_at_or_under(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// Editor tabs (user request): which tab becomes active after closing
/// `closing`. `None` when the closed tab wasn't active (nothing changes) or
/// it was the last tab. Prefers the next tab, falling back to the previous
/// one — the usual browser/editor behaviour. Pure — unit-tested below.
fn next_active_tab(tabs: &[PathBuf], closing: &Path, active: Option<&Path>) -> Option<PathBuf> {
    if active != Some(closing) {
        return None;
    }
    let idx = tabs.iter().position(|p| p == closing)?;
    if idx + 1 < tabs.len() {
        Some(tabs[idx + 1].clone())
    } else if idx > 0 {
        Some(tabs[idx - 1].clone())
    } else {
        None
    }
}

/// T-WS-EDITOR-002 spec change (user): the tree listing is a view filter,
/// decoupled from the open buffer. Decide what `start_load`'s marshal-back
/// does with the selection: returns `(new_selected, should_load)`.
///
/// - With an open buffer: highlight-only — remap to the open file's new row
///   (`restore`), or clear the highlight when the file isn't listed. NEVER
///   load (the buffer — dirty or clean — survives any tree rebuild).
/// - With no buffer yet (initial load): select + load the first file.
///
/// Pure — unit-tested below ("open buffer survives source switch").
fn restore_selection(
    has_open_buffer: bool,
    restore: Option<usize>,
    file_count: usize,
) -> (Option<usize>, bool) {
    if has_open_buffer {
        (restore, false)
    } else if file_count > 0 {
        (restore.or(Some(0)), true)
    } else {
        (None, false)
    }
}

/// Reconstruct a `TreeRow::Dir` row's full repo-relative path (T-WS-EDITOR-007
/// — the tree context menu's New File/Folder/Rename/Delete/Copy Path/Reveal
/// items on a directory row need it). `TreeRow::Dir` only stores a `name`
/// relative to its immediate parent row (possibly multi-segment, from
/// `file_tree`'s single-child-directory compression) — not the full path —
/// so this walks backward from `index` to the nearest preceding row at each
/// decreasing depth (its structural parent, guaranteed by the depth-first
/// pre-order flattening `file_tree::build_file_tree_opt` produces) and joins
/// the names with `/`. Returns `None` for an out-of-range index or a `File`
/// row. Pure — unit-tested below.
pub(crate) fn dir_path_for_tree_index(tree: &[TreeRow], index: usize) -> Option<PathBuf> {
    let mut depth = match tree.get(index)? {
        TreeRow::Dir { depth, .. } => *depth,
        TreeRow::File { .. } => return None,
    };
    let mut segments: Vec<SharedString> = vec![match &tree[index] {
        TreeRow::Dir { name, .. } => name.clone(),
        TreeRow::File { .. } => unreachable!(),
    }];
    let mut i = index;
    while depth > 0 {
        let target_depth = depth - 1;
        let mut found = false;
        while i > 0 {
            i -= 1;
            let row_depth = match &tree[i] {
                TreeRow::Dir { depth, .. } => *depth,
                TreeRow::File { depth, .. } => *depth,
            };
            if row_depth == target_depth {
                match &tree[i] {
                    TreeRow::Dir { name, .. } => segments.push(name.clone()),
                    TreeRow::File { .. } => return None, // malformed: parent must be a Dir
                }
                found = true;
                break;
            }
        }
        if !found {
            return None; // malformed tree — no ancestor at the expected depth
        }
        depth = target_depth;
    }
    segments.reverse();
    let joined: String = segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("/");
    Some(PathBuf::from(joined))
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
    let unstaged_paths: HashSet<&Path> = status.unstaged.iter().map(|f| f.path.as_path()).collect();
    let staged_paths: HashSet<&Path> = status.staged.iter().map(|f| f.path.as_path()).collect();
    for f in status.unstaged.iter().chain(status.staged.iter()) {
        if seen.insert(f.path.clone()) {
            files.push(WorkspaceFile {
                path: f.path.clone(),
                change: Some(f.change.clone()),
                untracked: false,
                unstaged: unstaged_paths.contains(f.path.as_path()),
                staged: staged_paths.contains(f.path.as_path()),
            });
        }
    }
    for p in &status.untracked {
        if seen.insert(p.clone()) {
            files.push(WorkspaceFile {
                path: p.clone(),
                change: Some(ChangeKind::Added),
                untracked: true,
                // Untracked files always show as unstaged (git status lists
                // them separately from `unstaged`/`staged`, but "Stage" is the
                // only action that applies until they're added).
                unstaged: true,
                staged: false,
            });
        }
    }
    files
}

/// Merge the full tracked+untracked path list (`Backend::worktree_files`)
/// with the working-tree change kinds (by path) for `TreeSource::All` — an
/// unmodified file simply gets `change: None` (no badge) and every
/// `untracked`/`staged`/`unstaged` flag `false` (a clean tracked file offers
/// none of the git-write tree-menu items), while a changed one keeps its real
/// flags so the badge + menu items still show (T-WS-EDITOR-004 scope item 3 /
/// T-WS-EDITOR-007).
fn merge_all_files(all_paths: Vec<PathBuf>, changed: &[WorkspaceFile]) -> Vec<WorkspaceFile> {
    let by_path: HashMap<&Path, &WorkspaceFile> =
        changed.iter().map(|f| (f.path.as_path(), f)).collect();
    all_paths
        .into_iter()
        .map(|path| match by_path.get(path.as_path()) {
            Some(f) => WorkspaceFile {
                path,
                change: f.change.clone(),
                untracked: f.untracked,
                unstaged: f.unstaged,
                staged: f.staged,
            },
            None => WorkspaceFile {
                path,
                ..Default::default()
            },
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

    /// True when closing/switching repo context would drop unsaved editor tabs.
    pub fn editor_workspace_any_dirty(&self, cx: &mut Context<Self>) -> bool {
        match self.editor_workspace.as_ref() {
            Some(ev) => ev.read(cx).any_dirty(),
            None => false,
        }
    }

    /// True when deleting `path` would close at least one dirty editor tab.
    pub fn editor_workspace_dirty_under(&self, path: &Path, cx: &mut Context<Self>) -> bool {
        match self.editor_workspace.as_ref() {
            Some(ev) => ev.read(cx).any_dirty_under(path),
            None => false,
        }
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
    /// and run the pending action. The entity's `select`/`reload_from_disk`
    /// reset `dirty` as part of navigating/reloading, so no separate
    /// "discard" step is needed here.
    pub fn confirm_editor_dirty_guard(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_dirty_guard_modal().cloned() else {
            return;
        };
        self.clear_editor_dirty_guard_modal();
        match modal.intent {
            EditorPendingIntent::Reload => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.reload_from_disk(cx));
                }
            }
            EditorPendingIntent::CloseTab(path) => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.close_tab_now(&path, cx));
                }
            }
            EditorPendingIntent::Close => {
                self.close_editor_workspace();
            }
            EditorPendingIntent::SwitchRepo(path) => {
                self.close_editor_workspace();
                self.switch_repo_by_path(&path, cx);
            }
            EditorPendingIntent::CloseRepoTab(path) => {
                self.close_editor_workspace();
                self.close_tab_by_path(&path, cx);
            }
            EditorPendingIntent::EnterRemoteView { host, root, snap } => {
                self.close_editor_workspace();
                self.enter_remote_view(host, root, (*snap).clone(), cx);
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
    // T-WS-EDITOR-002 §2: "● " prefix while the open buffer has unsaved
    // edits. Keyed off `open_path` (not the tree highlight) — after a source
    // switch the open file may not be listed, and the header is then the
    // only thing identifying the buffer (spec change).
    let selected_path = view.open_path.as_ref().map(|p| {
        let path = p.to_string_lossy();
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
                .child(SharedString::from(Msg::EditorWorkspaceBackToGraph.t())),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::EditorWorkspaceTitle.t())),
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
    // T-WS-EDITOR-007: right-click on the empty area below the tree (or the
    // whole pane while loading/empty) opens the menu targeting the repo root
    // (New File… / New Folder…). File/dir rows below stop propagation on
    // their own right-click, so this only fires for clicks that miss a row.
    let root_menu = cx.listener(|this, e: &gpui::MouseDownEvent, _w, cx| {
        this.open_tree_menu(TreeMenuTarget::Root, e.position, cx);
    });
    let mut pane = div()
        .w(theme::scaled_px(view.tree_w))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        .on_mouse_down(MouseButton::Right, root_menu)
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
            let right_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
                cx.stop_propagation();
                this.open_tree_menu(TreeMenuTarget::Dir(index), e.position, cx);
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
                    .on_mouse_down(MouseButton::Right, right_click)
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
            // T-WS-EDITOR-002 §2 + editor tabs: any row whose file is open
            // in a tab with unsaved edits gets the dot, active or not.
            let show_dirty_dot = view
                .files
                .get(file_index)
                .is_some_and(|f| view.tab_dirty(&f.path));
            let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.select(file_index, cx);
            });
            let right_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
                cx.stop_propagation();
                this.open_tree_menu(TreeMenuTarget::File(file_index), e.position, cx);
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
                    .on_mouse_down(MouseButton::Right, right_click)
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
/// The editor tab strip (user request): one chip per open buffer — file
/// name, `●` when dirty, and a × close button (dirty close routes through
/// the unsaved-changes guard). `None` when no tabs are open.
///
/// ponytail: no horizontal scrolling — many tabs just shrink/truncate.
/// Add an overflow scroll if real usage ever opens that many.
fn render_tab_strip(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<gpui::AnyElement> {
    if view.open_tabs.is_empty() {
        return None;
    }
    let mut strip = div()
        .id("ews-tabs")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .bg(rgb(theme().panel));
    for (i, path) in view.open_tabs.iter().enumerate() {
        let is_active = view.open_path.as_ref() == Some(path);
        let dirty = view.tab_dirty(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let activate_path = path.clone();
        let close_path = path.clone();
        let activate = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
            this.activate_tab(activate_path.clone(), cx);
        });
        let close = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
            // Don't also fire the chip's activate handler underneath.
            cx.stop_propagation();
            this.request_close_tab(close_path.clone(), cx);
        });
        strip = strip.child(
            div()
                .id(("ews-tab", i))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .py_px()
                .max_w(theme::scaled_px(180.))
                .min_w(px(0.))
                .cursor_pointer()
                .text_xs()
                .bg(rgb(if is_active {
                    theme().bg_base
                } else {
                    theme().panel
                }))
                .when(!is_active, |el| el.hover(|s| s.bg(rgb(theme().surface))))
                .on_click(activate)
                .when(dirty, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from("\u{25cf}")),
                    )
                })
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(rgb(if is_active {
                            theme().text_main
                        } else {
                            theme().text_sub
                        }))
                        .child(SharedString::from(name)),
                )
                .child(
                    div()
                        .id(("ews-tab-x", i))
                        .flex_shrink_0()
                        .px_1()
                        .rounded_sm()
                        .text_color(rgb(theme().text_muted))
                        .hover(|s| {
                            s.bg(rgb(theme().selected))
                                .text_color(rgb(theme().text_main))
                        })
                        .on_click(close)
                        .child(SharedString::from("\u{00d7}")),
                ),
        );
    }
    Some(strip.into_any_element())
}

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

    // Editor tab strip (user request: multiple open buffers).
    if let Some(strip) = render_tab_strip(view, cx) {
        pane = pane.child(strip);
    }

    // T-WS-EDITOR-002 §4: external-change banner — only when the buffer is
    // dirty (a clean buffer just silently re-reads via `on_worktree_changed`).
    // Reload replaces an unsaved edit with the on-disk text, so it routes
    // through the dirty guard (§5 spec change) rather than firing directly.
    if view.external_changed {
        let reload = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.open_dirty_guard(EditorPendingIntent::Reload, cx);
        });
        let overwrite = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.save_overwrite(cx);
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
                )
                .child(
                    div()
                        .id("ews-overwrite")
                        .px_2()
                        .py_px()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .bg(rgb(theme().bg_base))
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)))
                        .on_click(overwrite)
                        .child(Msg::EditorWorkspaceOverwrite.t()),
                ),
        );
    }

    // T-WS-EDITOR-005 finding #6: deleted/missing and non-UTF-8 files used to
    // both fall through to the generic "select a file" placeholder even
    // though a row was actually selected — distinguish why `content` is
    // `None` instead.
    //
    // T-WS-EDITOR-002 spec change: keyed off `open_path`/`content`, not the
    // tree state — a tree reload (`loading`, e.g. a source switch) and a
    // sig-guarded background re-read (`file_loading` with content already
    // present) must NOT blank an open buffer.
    let placeholder = if view.open_path.is_none() {
        Some(if view.loading {
            Msg::EditorWorkspaceLoading.t()
        } else {
            Msg::EditorWorkspaceSelectFile.t()
        })
    } else if view.file_loading && view.content.is_none() {
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
    fn dir_path_for_tree_index_reconstructs_nested_and_top_level() {
        let tree = sample_tree();
        // Top-level dir "src" (idx 0): its own name is already the full path.
        assert_eq!(
            dir_path_for_tree_index(&tree, 0),
            Some(PathBuf::from("src"))
        );
        // Nested dir "ui" (idx 2, depth 1, parent "src" at idx 0): "src/ui".
        assert_eq!(
            dir_path_for_tree_index(&tree, 2),
            Some(PathBuf::from("src/ui"))
        );
        // A File index is not a Dir row.
        assert_eq!(dir_path_for_tree_index(&tree, 1), None);
        // Out of range.
        assert_eq!(dir_path_for_tree_index(&tree, 99), None);
    }

    #[test]
    fn dir_path_for_tree_index_handles_compressed_top_level_name() {
        // build_workspace_tree's own compression test shape: Dir("a/b") at
        // depth 0 already contains the full compressed path.
        let files = vec![WorkspaceFile {
            path: PathBuf::from("a/b/c.rs"),
            change: None,
            ..Default::default()
        }];
        let tree = build_workspace_tree(&files);
        assert_eq!(
            dir_path_for_tree_index(&tree, 0),
            Some(PathBuf::from("a/b"))
        );
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
    fn path_is_at_or_under_matches_path_components() {
        assert!(path_is_at_or_under(
            Path::new("src/ui/main.rs"),
            Path::new("src/ui")
        ));
        assert!(path_is_at_or_under(
            Path::new("src/ui"),
            Path::new("src/ui")
        ));
        assert!(!path_is_at_or_under(
            Path::new("src/ui_extra/main.rs"),
            Path::new("src/ui")
        ));
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
            ..Default::default()
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

    // ── T-WS-EDITOR-007: staged/unstaged/untracked flags ───────────────────

    #[test]
    fn merge_working_tree_files_flags_untracked_staged_unstaged() {
        let status = kagi_git::WorkingTreeStatus {
            staged: vec![kagi_git::FileStatus {
                path: PathBuf::from("staged.rs"),
                change: ChangeKind::Modified,
            }],
            unstaged: vec![kagi_git::FileStatus {
                path: PathBuf::from("unstaged.rs"),
                change: ChangeKind::Modified,
            }],
            untracked: vec![PathBuf::from("new.rs")],
            conflicted: Vec::new(),
        };
        let files = merge_working_tree_files(&status);

        let staged = files
            .iter()
            .find(|f| f.path == Path::new("staged.rs"))
            .unwrap();
        assert!(staged.staged && !staged.unstaged && !staged.untracked);

        let unstaged = files
            .iter()
            .find(|f| f.path == Path::new("unstaged.rs"))
            .unwrap();
        assert!(!unstaged.staged && unstaged.unstaged && !unstaged.untracked);

        let untracked = files
            .iter()
            .find(|f| f.path == Path::new("new.rs"))
            .unwrap();
        assert!(!untracked.staged && untracked.unstaged && untracked.untracked);
    }

    #[test]
    fn merge_all_files_preserves_flags_for_changed_entries() {
        let changed = vec![WorkspaceFile {
            path: PathBuf::from("src/a.rs"),
            change: Some(ChangeKind::Modified),
            untracked: false,
            unstaged: true,
            staged: false,
        }];
        let all = vec![PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")];
        let merged = merge_all_files(all, &changed);

        let a = merged
            .iter()
            .find(|f| f.path == Path::new("src/a.rs"))
            .unwrap();
        assert!(a.unstaged && !a.staged && !a.untracked);
        let b = merged
            .iter()
            .find(|f| f.path == Path::new("src/b.rs"))
            .unwrap();
        assert!(!b.unstaged && !b.staged && !b.untracked);
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
                ..Default::default()
            },
            WorkspaceFile {
                path: PathBuf::from("top.txt"),
                change: Some(ChangeKind::Added),
                ..Default::default()
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
    fn next_active_tab_prefers_next_then_previous() {
        let tabs: Vec<PathBuf> = ["a", "b", "c"].iter().map(PathBuf::from).collect();
        let (a, b, c) = (Path::new("a"), Path::new("b"), Path::new("c"));
        // Closing an INACTIVE tab: the active tab doesn't change.
        assert_eq!(next_active_tab(&tabs, a, Some(b)), None);
        // Closing the active middle tab → the next one.
        assert_eq!(next_active_tab(&tabs, b, Some(b)), Some(PathBuf::from("c")));
        // Closing the active last tab → the previous one.
        assert_eq!(next_active_tab(&tabs, c, Some(c)), Some(PathBuf::from("b")));
        // Closing the only tab → nothing left.
        let only = vec![PathBuf::from("a")];
        assert_eq!(next_active_tab(&only, a, Some(a)), None);
    }

    #[test]
    fn open_buffer_survives_source_switch() {
        // T-WS-EDITOR-002 spec change (user): a tree rebuild (source switch /
        // watcher / post-save) never loads over an open buffer.
        //
        // Open file still listed → remap the highlight, no load.
        assert_eq!(restore_selection(true, Some(3), 10), (Some(3), false));
        // Open file NOT in the new list (All→Changes) → clear the highlight,
        // keep the buffer (no load — `select` would clobber it).
        assert_eq!(restore_selection(true, None, 10), (None, false));
        // Even with an empty list the buffer stays.
        assert_eq!(restore_selection(true, None, 0), (None, false));
    }

    #[test]
    fn initial_load_selects_and_loads_first_file() {
        // No buffer yet: select + load the restored (or first) file.
        assert_eq!(restore_selection(false, None, 5), (Some(0), true));
        assert_eq!(restore_selection(false, Some(2), 5), (Some(2), true));
        // Empty worktree list: nothing to select.
        assert_eq!(restore_selection(false, None, 0), (None, false));
    }
}
