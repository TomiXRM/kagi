# ADR-0016: Changed Files Tree/List and Diff Viewer

- Status: Accepted / Date: 2026-06-12(T018/T-UI-002/003/004 の決定を統合追認)

## Decision
- Changed Files は flat(Path)⇄ Tree をトグル可能(Inspector 側にも導入。Commit Panel は導入済み)。両ビューで status バッジ・件数・選択ハイライト・active ファイル強調を共通仕様とする
- Diff は main pane 全幅(T-UI-003)・行番号・tree-sitter ハイライト(T-UI-004)・binary/renamed/deleted の明示表示
- **大 diff**: 行数 > 2000 で hunk 単位の fold(初期は先頭 N hunk 展開 + 「Show more」)。それ以下は全展開
- commit diff(HEAD↔parent)と WIP diff(index↔WT / HEAD↔index)は MainDiffSource で区別(導入済み)

## Consequences
- fold 状態は MainDiffView 内の一時状態(永続化しない)

## Amendment — 2026-10-12: keyboard diff selection and hunk actions (#1233 / #1234)

- The shared diff body is a Tab stop and takes focus when a row is clicked.
  Only that focused body owns Up/Down caret movement, Shift+Up/Down range
  extension, Shift+Cmd+Up/Down extension to the first/last diff row and Cmd+A.
  Child controls retain their own keyboard behavior. Escape remains in the
  workspace cancellation chain: selection clears before notification dismissal.
  Its FocusHandle and element identity use only the title, independently of the
  row-count-dependent selection token, so same-file reloads retain focus.
  A 2px accent overlay makes keyboard focus visible without resizing the list
  or rows; pointer input suppresses it until the next keyboard input.
- Keyboard selection reuses `diff_selection`'s single title/count surface token,
  inclusive anchor/cursor range and `selection_overlay` paint. Moving a caret
  without Shift collapses the range to that row; Shift preserves the anchor.
  On a fresh surface the first display row is the starting caret position.
  Hunk headers may anchor a range, but copy still excludes headers/binary rows
  and strips sigils/line numbers. Keyboard changes eagerly refresh stored copy
  text; mouse drag and mouse-up extraction retain their existing semantics.
  Header tint identifies only a keyboard caret; mouse-selected headers retain
  their original surface fill.
  Header paint probes run after the background is applied. The process-global
  model retains std Mutex with poison recovery, without a new dependency (#1080).
- The existing native ListState reveals the caret. Unified row indices are
  mapped through the cached split projection and optional expansion-item
  layout before scrolling; no independent scroll state is introduced.
- Stage/Unstage hunk use the pinned gpui-kit ghost/small Button, localized
  caption/AX name and Tooltip. Natural height and existing horizontal padding
  preserve the dense hunk-header geometry instead of imposing toolbar height.
  Tab focus, Enter and Space use the same callback and frozen HunkApproval as
  pointer activation (#1131); the existing backend staging pipeline is unchanged.
- Tier A scenarios `diff_hunk_buttons` and `diff_keyboard_selection` cover
  declared role/name, measured geometry, traversal/activation, selection fill,
  code-only copy and cancellation. They do not claim native VoiceOver delivery.


## Amendment — 2026-10-12: full header identity (#1336)

The shared DiffHeader title remains the complete sanitized original path.
Only its one-line paint truncates: a Label node exposes that same full value
as its description and the pinned Tooltip reveals it on hover. Title shrink
priority, mode/action/stat geometry and the hunk viewport do not change.
Renderer-fed metadata and invoked-tooltip/native-bound Tier A coverage do
not claim native VoiceOver delivery.

## Amendment — 2026-10-13: Commit Panel file-list density (#1463)

Staged and Unstaged retain the existing virtualized `uniform_list`, retained
RowFocus, row/action Tab stops and TreeItem level/sibling metadata. All file,
directory and disclosure slots are explicitly 22 reference px, scaled once at
render time with a 20-logical-px floor, rather than inheriting the inline
Stage/Unstage button's natural height. Directory labels are vertically centered
in that same slot: a shorter top-aligned label previously left its spare height
only below the directory.
The native regression recorded the old file pitch as 26px at 100%.

Inline Stage/Unstage uses a scaled 20-reference-px height with its existing
20-logical-px minimum and no extra vertical padding. Its reserved 2px
focus-visible border is inside the same clickable frame: the kit's outside
ring would extend into, and be overpainted by, the next uniform slot.
Badge, filename, directory, disclosure and shared diffstat numbers use
`FILE_TREE_FONT_PX` (13 reference px), with a compact 16-reference-px line height
and badge column. The shared diffstat change also makes Inspector's diffstat
numbers 13 reference px, consistent with `hig/inspector-scale`'s shared token;
Inspector layout and Graph density are otherwise unchanged. Section-header
padding becomes 1px per side instead of `py_1`; the first list row still starts
immediately inside the list's existing border.

Role/reference: dense changed-file list; take Finder list view's 20pt pitch,
13pt type and 16px icon rhythm, not its operation semantics or selection policy.
Reuse the native uniform-list, shared KagiButton, RowList and tree accessibility helpers rather than another
scroll/focus owner. Rest/hover/selected/focus-visible/conflicted/empty states,
localized names, session-owned reads and staging approval/execution are unchanged.
`commit_panel_density` measures both sections in List/Tree at 940×660 and
1392×883, 100%/167%, EN/JA, and checks native pixels of the remembered row's
inline action's lower focus edge after Tab, with the adjacent row also painted.
Native attribute bounds are not VoiceOver evidence.

## Amendment — 2026-10-14: layout-only header measurement (#1523)

HeaderFit still measures the actual labelled pinned Buttons at the current
language and zoom. Its private measurement Element delegates request_layout
to that copy but never delegates prepaint or paint: natural bounds update
the existing fitting state without registering accessibility nodes, focus,
hitboxes or drawing. Offscreen placement and disabled Tab stops alone did not
exclude the copies from the native accessibility tree. Real header actions,
title priority, geometry and icon-only fallback are unchanged.
`diff_header_ax_unique` inspects the activated native AccessKit tree in EN/JA,
narrow icon-only and wide labelled layouts; every header action appears once
and no measurement-copy node is present.

## Amendment — 2026-10-15: batch staging publication (#1569)

Stage all / Unstage all retain the existing admitted, exact-path index
mutation. Their post-write read now publishes the panel and the owning
session's status together, retaining that same status as the watcher baseline.
The WIP badge reuses the panel's already-computed staged/unstaged diffstats;
advancing its request revision prevents an older background badge from landing.
A linked-worktree panel updates only its matching worktree row, never the
active repository's status or badge.

This is a read/publication correction, not a staging executor change.
`working_tree_status` remains pure, the watcher retains its debounce and
stat-cache repair policy, and no graph snapshot, ahead/behind or stash read
is added to the local index-write path. Existing receipt behavior is unchanged.
The deterministic `batch_staging_refresh` native scenario checks 17 and
200 paths' immediate panel/session agreement before background tasks or a
watcher can run, with unchanged working-file bytes and expected index trees.

