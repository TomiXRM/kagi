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

## What ended up shared with File History (revised after review)

The first cut of this pane rendered its own commit rows and reimplemented
the surrounding behaviour. That was the wrong default and it showed: five
successive user reports were each a variant of "File History already does
this" — an older commit predating a rename resolved against today's path
and displayed nothing; no A/M/D/R badge; no commit subject/body header; no
author/co-author avatars; the UI font in code panes. Each was fixed by
going back and sharing the thing that already existed.

What is shared now, and by whom:

| piece | home | consumers |
|---|---|---|
| `FileHistory::entry_by_hash` | `kagi-domain::file_history` | both panes |
| `load_history_entry_file_diff` | `src/ui/file_history.rs` | both panes |
| `render_commit_header` (subject/meta/body/co-authors + avatars) | `kagi-ui-core::commit_header` | both panes |
| `entry_badge` / `change_type_badge` / `change_type_label` | `kagi-ui-core::change_badge` | both panes |
| `avatar_color` / `avatar_initial` / `AvatarImages` | `kagi-ui-core::avatar` | both panes + bin |

File History gained from the consolidation too — its detail pane now shows
the richer shared header (reflowed body, co-authors, avatars) instead of a
plain "Message" row.

The row rendering itself is shared too, via
`kagi-ui-core::commit_row`. The two panes genuinely need different shapes —
File History's full-width six-column table vs. the Editor tab's two-line
card in a ~380px sidebar — so *the layout is the parameter*
(`CommitRowLayout::{Table { row_height }, Card}`) and everything feeding it
is common: `commit_row_model` derives every display string once (badge,
subject, author, both date formats, `+ins −del`, short hash) and
`row_background` owns the zebra/selection rule both lists had copy-pasted.

`render_commit_row` returns a `Stateful<Div>` rather than a finished
element, so each pane attaches its own interactions — File History a
double-click-to-jump plus a right-click context menu, the Editor tab a
single click that drives the *separate* center pane. That keeps the
handler differences out of the shared code entirely, instead of growing
callback parameters only one caller ever passes. Net effect: the two row
functions are now 48 and 29 lines (from 160 and 149), and both are almost
entirely their own interactions.

**Guidance for the next pane:** when a new pane needs something an existing
pane already renders, extract the shared piece *first* and have both
consume it. Do not write a second renderer and back-port features as bug
reports arrive. ADR-0121's rule that sibling `kagi-ui-*` crates cannot
depend on each other makes duplication the path of least resistance — which
is exactly why the extraction into `kagi-ui-core` has to be a deliberate
first step rather than a cleanup afterwards.

## Consequences

- `Backend::blob_bytes_at` and `file_content_at_commit` now share one
  resolution helper instead of two copies of the same commit/tree/path walk.
- The center pane gained a second, independent diff surface
  (`history_diff`); any future third diff embedding in this entity has a
  template to copy rather than improvising a new opacity trick.
- ADR-0089's center-pane File History takeover keeps its behaviour, but its
  detail pane and badge helpers now come from `kagi-ui-core` (shims left in
  place, so `kagi_ui_file_history::entry_badge` etc. still resolve).
- `render_diff_list` sets `MONO_FONT` on the diff body: gpui-component's
  `code_editor()` never applies a monospace face (its `mono_font_family` is
  only read by that crate's own inspector and markdown blocks), so every
  code surface in kagi must set the font itself. Known remaining duplicate:
  `src/ui/inspector.rs::change_badge` is a third copy of
  `kagi_ui_core::file_tree::status_badge` — flagged, not yet merged.
