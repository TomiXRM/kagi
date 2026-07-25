# ADR-0132: Editor Workspace — right-pane History tab, center-pane Diff/Snapshot tabs

- Status: Accepted
- Date: 2026-07-25

## Context

Editor mode's right pane always showed the open file's WIP hunks, with no
alternative view. User request: let the right pane switch to that file's
commit history, and let selecting a commit there switch the center pane
between that commit's diff and a read-only snapshot of the file's full text
at that point.

The center pane already renders the WIP text via `content`/`editor` — a live,
saveable buffer (T-WS-EDITOR-002: dirty tracking, Cmd-S save, undo history).
Whatever shows a historical commit's content must not touch that buffer.

ADR-0089's file-history data layer (`kagi_git::file_history`, `git log
--follow`) already exists and is fully built, but only as a full-width
center-pane takeover (`kagi-ui-file-history`). That feature is unrelated in
UI shape (this is a ~380px sidebar list, not a takeover) and stays untouched;
only its data layer is reused here.

## Decision

### New read-only primitive: file content at a commit

`Backend::file_content_at_commit(id, path) -> Result<Option<FileSnapshotContent>, GitError>`
(`crates/kagi-git/src/backend.rs`), built on the same commit → tree → path →
blob resolution `blob_bytes_at` already used — both now share a private
`find_blob_at` helper rather than duplicating that walk. Binary detection
reuses `git2::Blob::is_binary()` plus `kagi_domain::checklist::content_looks_binary`
(the same two-part heuristic `checklist.rs`'s `blob_is_binary` already applies
to staged blobs). `FileSnapshotContent { content: Option<String>, is_binary: bool }`
lives in `kagi_domain::file_history` next to `FileHistoryEntry` — pure data,
no git2. This is a read-only lookup; the `plan/confirm/preflight/execute/verify/oplog`
pipeline does not apply.

### State lives directly on `EditorWorkspaceView`, mostly untyped-free

`kagi-ui-editor`'s `Cargo.toml` already declares `kagi-domain` as a direct
dependency ("status/diff types... never the Git backend crate"). Since
`FileHistory`/`FileHistoryEntry`/`CommitSummary`/`FileSnapshotContent` are all
pure `kagi_domain` types, the crate names and renders them directly —
`history: Option<FileHistory>` and `snapshot: Option<FileSnapshotContent>` are
typed fields, not `Box<dyn Any>`. Only the *diff* view needs the existing
opacity trick: `history_diff: Option<Box<dyn Any>>` holds a bin-owned
`MainDiffView`, downcast by a new `EditorHooks::render_history_diff` hook —
the same shape as the pre-existing `render_hunks`/`diff` pair, kept as a
second, independent field/hook/scroll-state trio so selecting a History
commit never touches the right pane's WIP diff.

`RightPaneTab::{Diff, History}` and `MiddlePaneTab::{Diff, Snapshot}` gate the
two switchers. The center pane only shows History content when **both**
`right_tab == History` **and** a commit is selected — flipping the right pane
back to Diff reverts the center pane to the normal WIP view too, without
discarding `history`/`selected_history_commit`/`history_diff`/`snapshot`
(flipping back to History instantly re-shows them, no reload). Loads are
lazy per tab (`set_right_tab`/`set_middle_tab` only emit a request when the
target field is still empty) and guarded by the existing `file_req` token
plus a `selected_history_commit` equality check, so a rapid-fire commit
selection can't let a stale load land after a newer one — no new counter
types were needed for that.

History/Snapshot state is not tab-cached (`EditorBufferState`/`tab_cache`) —
it resets on every `open_tab` call, same lifetime as the preview-markdown
flag right next to it. Per-tab persistence wasn't requested; this can be
upgraded later the same way tab-cached WIP diffs already are, if asked for.

### History list request shape

`FileHistoryRequest { follow_renames: true, include_wip: false, limit: 500 }`.
`include_wip: false` is deliberate: the synthetic WIP row has no commit hash
to select, and the Diff tab in the very same right pane already shows the WIP
state, so there is nothing to gain from listing it a second time — every row
in the History list is guaranteed to be a real, clickable commit.

### Rendering

New sibling module `crates/kagi-ui-editor/src/panes.rs` (`lib.rs` is already
past the 800-LOC target; CLAUDE.md's guidance is a focused sibling over
growing it further). The History list and the Snapshot text both reuse the
same `uniform_list` virtualization the left file tree already uses (fixed
row height, unlike the diff list's variable-height `gpui::list`). The
commit-diff tab reuses `render_diff_list` exactly as `render_hunks` already
does. `MainDiffSource::Unstaged { path }` is reused for the historical
commit diff (not a new `Commit`-shaped variant) — the same choice
`FileHistoryView::load_diff` already made for its own per-commit diff, since
neither embedding wires up `MainDiffSource`'s image-preview stepping context.

## Consequences

- `Backend::blob_bytes_at` and `file_content_at_commit` now share one
  resolution helper instead of two copies of the same commit/tree/path walk.
- The center pane gained a second, independent diff surface
  (`history_diff`); any future third diff embedding in this entity has a
  template to copy rather than improvising a new opacity trick.
- No new ADR-0089 changes — the center-pane File History takeover is
  unaffected by this work.
