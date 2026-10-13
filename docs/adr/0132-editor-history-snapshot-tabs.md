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
`history: Option<Arc<FileHistory>>` and `snapshot: Option<FileSnapshotContent>` are
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

The accepted file-history read owns one immutable `Arc<FileHistory>`. Rendering
retains that same snapshot in the visible-range callback rather than cloning
all entries, commit messages and bodies on each redraw. Replacing or clearing
the file read replaces or clears the snapshot; the domain `FileHistory` remains
a plain owned value, and the existing file/request ownership guards still
control publication.

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

## Amendment — 2026-10-12: immutable selectable historical content (#1264)

Historical snapshots must not own an `InputState`. The previous independent
code editor accepted input without dirty tracking or a persistence owner:
its edits appeared real until switching revisions discarded them. The pinned
gpui-kit Input has no read-only-but-selectable mode; disabling it also prevents
the focus/selection needed for copy. Do not vendor or fork the widget for this
surface.

Use the existing selectable `TextView` for literal monospace code, with encoded
content prepared once when the accepted snapshot is installed. No typing,
paste, cut, delete, undo or IME mutation path exists, and no insertion caret is
drawn. The working-tree Input and its save/dirty owner remain independent.

The snapshot pane header exposes an EN/JA read-only indicator with historical
file path and full revision SHA to accessibility and its tooltip. Two bounded
rows keep the read-only title separate from the truncated identity and named
`Open current` Button, rather than allowing a full SHA to consume the body
height at high zoom. Open current returns to the already-owned live buffer,
without reloading or discarding edits. No restore action exists on this Editor
surface and none is added: historical content must not become a working-tree
write outside the existing approved write lifecycle.

The Editor's bottom-panel presentation cap reserves 200 reference pixels for
the compound workspace header, file tabs, history selector, snapshot identity,
and at least two source lines, after subtracting titlebar/toolbar/status chrome.
Fitting never overwrites the preferred panel height. The native snapshot
oracle retains its 60px readable-body floor at 940×660 / 167%, including a
166-reference-pixel preference retained from a desktop window. Header styling,
Button hit targets, terminal ownership and the live buffer remain unchanged.
Pointer resizing uses the same presentation cap, preserves its measured press
origin through clamp overshoot, and restores the larger preference when the
pointer returns to that origin. Its native oracle independently calculates the
mode-specific cap from the viewport, chrome, and compound Editor budget, checks
overshoot against that cap, and requires visible movement whenever room remains.

Horizontal Editor fitting measures its actual body bounds, excluding the host's
Compact icon rail. Both side-pane presentation widths and divider inverse
coordinates use those bounds, preserving the preferred widths while reserving
180 reference pixels for a fully revealed, maximum-width buffer tab. The
out-of-flow measurement requests a new frame only when horizontal geometry
changes; it does not override manual tab scrolling on ordinary redraws.

Copy retains the pinned selectable TextView's existing clipboard semantics:
the shared Root trims outer whitespace, including the final newline. Snapshot
source content remains unchanged; no local clipboard override or second
selection owner is introduced. Native coverage checks the copied literal code
before and after editing attempts and separately compares the complete raw
snapshot content.

The snapshot body additionally has an `EditorSnapshot` key context, but key
predicates alone are not a safety boundary: selecting a revision leaves root
focus, and its header Button is outside the body. Save and repository Undo/Redo
handlers therefore require foreground `WorkspaceMode::Editor` and
`showing_history_commit()` (History right pane with a selected commit),
independent of focus. A retained historical Editor behind PRs, Issues or a
takeover must not block those surfaces' normal shortcuts. Native coverage
switches from a selected historical commit to PRs and opens its real Undo modal.
A dirty live buffer in the foreground historical Editor must remain
dirty and unchanged on disk before and after clicking the reader. Native
coverage retains a real undo-capable history entry and verifies no modal opens.
Open current leaves historical state with the same live Input focused.

Native Save availability and dispatched writer admission are separate:
menus remain disabled while busy, but Cmd-S stays registered for a visible live
buffer and reaches the existing lease refusal/footer/toast path.
Hidden or historical buffers cannot save. Ordinary modal routing remains
inert; a reconciled panic notice must be dismissed before retrying.

For LF content, each literal block contains at most 32 source lines so
TextView's existing `ListState` virtualizes chunks rather than laying out the
whole file on every frame. Syntax highlighting is disabled above 128 KiB or
2,048 lines; initial
parsing/measurement is still proportional to source size. Native coverage
copies the complete 20,000-line fixture, then compares median warm forced
TextView redraws against a 102-line fixture (8× plus 15 ms allowance, not the
196× source growth).
Small-file highlighting follows the kit's block-local behavior; multiline syntax
context is not carried across chunk boundaries.

CRLF content deliberately uses a single literal block: Markdown consumes the CR
before a closing fence, so splitting on line boundaries would silently replace
those source separators with LF in copied text. The native 102-line CRLF leg
compares complete clipboard content across the former 32-line boundaries.
This correctness fallback does not provide bounded warm layout for large CRLF
files; the 20,000-line performance claim applies to LF content only.

Accepted presentation trade-off: this pinned selectable reader loses the former
CodeEditor's line-number gutter and horizontal no-wrap scrolling; long lines
wrap. Genuine immutability and the shared selection/copy owner take precedence
over an editable-looking viewer or a fork. [#1275](https://github.com/TomiXRM/kagi/issues/1275)
tracks restoring these capabilities, preferably by aligning a line-number
column with source rows/chunks in the same `ListState`, never an independent
scroll or selection owner.

## Amendment — 2026-10-12: Find/Replace accessibility (#1348)

The editable code buffer keeps gpui-kit's built-in SearchPanel: Kagi does not
introduce a second query, matcher, text input or replacement owner. The fork
provides `InputState::search_labels` / `set_search_labels`: standalone replacement
captions keep their existing rust_i18n translation and `Replace All` spelling,
while the new default icon names are English. `kagi-ui-core::find` supplies EN/JA
`Msg` names to every code-editor caller (Editor, Conflict Result and Issue body),
including retained language changes without resetting query or buffer.
Previous/Next, case, replacement disclosure,
Close, Replace and Replace all remain real Buttons with their existing
callbacks, icon geometry, unavailable states and theme styling. Accessible
Button names and lazily constructed hover help share the localized text;
case and replacement disclosure additionally expose `aria_toggled`.
Both `gpui-component` and `gpui-component-assets` are pinned to the published
TomiXRM/gpui-kit revision `ac2f8ceb088541251d0badca4b81743b5c745c65`; the
production dependency graph contains no local development path patch.

The search field precedes case/disclosure/navigation/Close in focus traversal;
the replacement field precedes Replace/Replace all. Escape from any Find-bar
control closes it and focuses its owning editable buffer, as does Close.
The component binds Escape for its `SearchPanel` context as well as Input;
Kagi's global CloseMainDiff binding excludes that context, so it cannot consume
the key before the Find control's action handler.
The bar has a separate non-Tab-stop container focus handle, rather than reusing
the field's handle. Its Buttons' Return key suppresses the ancestor Input's
Find Next binding so the normal GPUI keyboard click invokes the named action.
When replacement can disable its focused action, focus goes to the replacement
field rather than leaving Escape on an untracked control.
Find Buttons and the bar consume inherited editing/selection/movement actions
before they can reach the enclosing code Input. The case Button also guards
its enclosing query Input. A mouse-down on unclaimed padding focuses the query,
preserving either field's normal mouse selection and never selecting underlying
code. The Issue body's existing capture-stage fenced-paste handler only runs
when the body itself is focused, not when a descendant Find control is focused.
Input shortcuts, IME handling, replacement history and the host-owned guarded
Save path remain unchanged. No Git/backend dependency enters the Editor crate.

Tier A observes assignments from the actual component renderer, native bounds,
invoked Tooltip builders, rendered focus traversal and simulated activation.
Its matrix is 940×660 / 1392×883, EN/JA, 100%/167%, Apple Light/Dark.
The ownership regression records a before-FAIL when Button-focused Backspace
clears the hidden buffer, then checks editing refusal and padding focus. Actual
Issue-body and Conflict-Result legs check initial/live EN/JA names and help.
Native VoiceOver, Full Keyboard Access and live IME are separate PM-owned
Tier B checks, not claims from these renderer/TestDispatcher observations.
