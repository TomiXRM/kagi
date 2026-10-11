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

