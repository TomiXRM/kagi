# ADR-0029: Command Registry とメニューバー

- Status: Accepted / Date: 2026-06-12

## Decision

- **Command Registry を単一の正準とする**。メニューバー / コンテキストメニュー / ツールバー /
  ショートカット /(将来の)コマンドパレットは全てここを参照し、処理の二重実装を禁止する:
  ```rust
  pub struct Command {
      pub id: &'static str,            // "file.openRepository"
      pub label: &'static str,         // "Open Repository…"
      pub keystroke: Option<&'static str>, // gpui 記法 "cmd-o"(macOS は cmd)
      pub dangerous: bool,             // 将来の赤表示・二段階確認用属性
  }
  pub enum CommandState { Enabled, Disabled(&'static str), Hidden }
  pub fn command_state(app: &KagiApp, id: &str) -> CommandState   // 状態判定の一元化
  ```
  実装は `src/ui/commands.rs` に集約。`enabled`/`visible` はクロージャではなく
  `command_state` 関数1本に寄せる(状態判定を UI コンポーネントに散らさない)
- **メニューバーは gpui ネイティブ**: `cx.set_menus(Vec<Menu>)` +
  `MenuItem::action / os_action / separator / submenu`。ショートカット表示は keymap から
  自動(`KeyBinding` 登録が同時に表記になる)
- **command ↔ gpui Action は 1:1**: `actions!(menu, [OpenRepository, NewTab, ...])`。
  menu item・KeyBinding・`on_action` ハンドラはすべて同じ action 型を使う
- **disabled は「ハンドラ未登録」で表現する**: macOS のメニュー検証は dispatch tree に
  action ハンドラが存在するかで enabled/disabled を決める(gpui mac 実装準拠)。
  root 要素で `.when(command_state(..)==Enabled, |el| el.on_action(...))` と条件登録すれば
  メニューが自動で灰色になる。menu の再構築(set_menus 再呼び出し)は不要
- **handler の実体は既存経路**: dangerous / 状態変更系は必ず既存の
  plan → confirm → preflight → execute → verify → oplog に乗せる(メニューから直接実行しない)。
  Commit メニューは `dispatch_commit_action`(ADR-0022)をそのまま呼ぶ
- **Edit メニューは OS 標準**: `MenuItem::os_action`(Undo/Redo/Cut/Copy/Paste/SelectAll)。
  グローバル KeyBinding は張らない(テキスト入力の標準動作を壊さないため)
- macOS の **app menu(先頭 Menu)**に About と Quit を置く
- 未実装機能の方針: 機能が存在しないものは `Disabled(理由)` で見せる(Clone / Zoom /
  Rename Branch / New Window 等)。隠すのは context 上ありえない項目のみ

## Consequences

- ツールバー(既存 Pull/Push 等)の registry への移行は段階的(本 ADR では新設分のみ必須、
  既存ボタンの置き換えは follow-up)
- コマンドパレット(cmd-shift-p)は registry がそのまま供給源になる(later)
- KeyBinding は root focus 必須(既存制約)。menu 起動はメニュー側 dispatch なので
  フォーカスが input にあっても動く

## Amendment (2026-10-08): palette selection follows the viewport

Issue #1069: `Cmd/Ctrl+P` の結果を矢印で選ぶと、選択が表示範囲外へ進んでも
scroll offset が変わらず、Enter の対象を目で確認できなかった。
実ウィンドウの空 query → Down 24 回で before / after を比較した。

- 結果リストは既存の可変行高の div を維持する。disabled 理由を切らず、
  `MenuKeys` と同じ pinned GPUI `ScrollHandle::scroll_to_item` を使う。
- handle は palette を初めて開くまで確保しない。再度開くときは既存 handle の
  offset と選択を先頭へ戻し、古い選択の reveal request を上書きする。
- arrows と query 更新だけが、一回限りの selected-child reveal を予約する。
  新しい children の実際の bounds に対して prepaint で適用するため、
  検索結果の減少・空結果からの復帰・翻訳・zoom で固定行高を仮定しない。
- 通常の render は reveal を予約しない。マウスで選択行から離れてもその
  scroll を維持し、次の矢印操作で選択先を表示する。先頭・末尾で index が
  変わらない矢印にも同じ契約を適用する。
- live results の長さが変わったときも、highlight と Enter は同じ current
  result index に clamp する。空結果・disabled は実行しない。
- command registry と既存の operation lifecycle、IME、Escape の return focus、
  MouseDown の action dispatch は変えない。palette の viewport geometry は
  #1084 の別監査で扱う。


## Amendment (2026-10-11): palette card viewport cap (#1084)

Role は dense search/command overlay。Reference は Spotlight の検索欄を残し、
結果だけを scroll する階層（native material や Git command semantics は移さない）。
既存 Input、ScrollHandle、gpui-component Tooltip を再利用し、新しい control は作らない。

`modal_shell::search_card_geometry` が top-aligned search card の geometry を所有する。
幅は min(560 scaled px, viewport − 32 scaled px)、高さは
min(480 scaled px, viewport − top − 16 scaled px)。top は 80 scaled px を
上限に window 高の 10% へ縮む。検索欄は shrink せず、結果だけが独立して scroll する。
long label は truncate し完全な名前を tooltip／AX に保持、shortcut は shrink しない。
結果は既存 `list_a11y` の ListBox／ListBoxOption とし、selected・全 label・
disabled reason の description を設定する。role のない div の name には依存しない。
native recorder は長い option の AX name、実 label bounds と single-line 高さを検証する。
measurement probe は gui-e2e のときだけ生成し、release render では名前を format しない。

rest／hover／selected／disabled／empty／many results と既存 Arrow／Enter／Escape
は維持する。loading／新しい error state は本変更にはない。IME と real-window
focus／VoiceOver delivery は Tier A の範囲外で PM の Tier B 確認対象。
native regression は 700×500@100%／167%、900×600@167% の EN／JA、
多数結果と長い custom theme 名で card／input／results bounds、独立 scroll、
shortcut の水平 containment を測る。既存 selection-scroll regression も維持する。

## Amendment (2026-10-12): one palette target and native shortcuts (#1271 / #1272)

- Spotlight-style selection remains `command_palette_selected`: arrow input and
  actual pointer movement update the same index. Only the selected row paints
  `selected`; there is no independent hover wash of equal strength.
- Opening captures the pointer's current position. Row movement compares against
  the last position tracked across the panel before updating selection. Replayed
  stationary coordinates, scrolling content and passive redraws never select.
  Pointer selection does not request scroll/reveal, preserving #1069.
- Enter reads the highlighted current result; disabled results can be highlighted
  with their existing localized reason but remain inert. Existing Escape return
  focus, MouseDown dispatch and operation safety paths remain unchanged.
- `commands::display_keystroke` stays the display owner. On macOS it parses the
  effective GPUI binding and delegates to pinned gpui-kit `Kbd::format`, including
  native modifier order ⌃⌥⇧⌘ and named keys. Non-macOS retains text notation.
  The palette reserves its existing shrink-free right-aligned trailing column.
- EN/JA command labels and disabled reasons keep their existing `Msg` owners;
  bindings and platform glyphs are language-independent, not translated prose.
  Native `palette_highlight` and `palette_shortcut_glyphs` cover 940×660 /
  1392×883, 100% / 167%, EN / JA and Apple Light / Dark. Real pointer/focus,
  IME and VoiceOver delivery remain PM-owned Tier B checks.

## Amendment (2026-10-13): empty-query sections (#1370)

The dense search overlay borrows related-command headings from macOS menus
([HIG Menus](https://developer.apple.com/design/human-interface-guidelines/menus)),
not native materials or a new command taxonomy. Empty queries show Actions,
Appearance and Language as EN/JA headings and named `Group` ancestors. The
palette has no recent-command history, so no Recents section or storage is added.

Headings are static text, not list options, focus stops or activation targets.
The selectable model remains commands followed by every registered theme and
language exactly once; Up/Down/Enter and pointer selection retain their existing
indices. Filtered queries remain a single score-ranked list; option descriptions
include their localized section and preserve the complete unavailable reason.

Semantic nesting must not turn a whole section into the scrolling destination:
measure descendant row layout bounds and honor one-shot reveal before GPUI
prepaint applies the container offset. Wheel scroll/passive redraw remain independent of selection; query
changes and boundary arrows still reveal the current real row. Shortcut captions
and the shrink-free trailing column retain #1271/#1272's owners.

Review correction: for a requested section-first row, capture its Group layout
ID as well as the selectable leaf. Upward reveal aligns the group top so the
heading is not clipped; requesting index zero restores offset zero, including
after clearing a query. Other destinations still reveal only their actual row.
Description joining belongs to UI-core i18n: EN `{section}. {reason}` and
JA `{section}。{reason}`, keeping the unavailable reason verbatim.

States: static headings have rest only; hover, pressed, focus, selected and
disabled are not applicable. Existing rows retain selected/disabled/empty/many
results behavior, with no new loading or error state. Geometry and spacing use
the palette's theme tokens and scaled pixels. `palette_sections` covers heading
bounds, Group/name inputs, row uniqueness, section-boundary keyboard navigation
and filtered ranking. Real-window Tab, IME and native VoiceOver remain Tier B.

## Amendment — 2026-10-13: reachable Keyboard Shortcuts help (#1441)

Role: dense read-only help overlay, taking macOS help's pinned heading and
independently scrollable content rather than changing command semantics.
The card retains its existing visual chrome, capped to the actual viewport
at high zoom. The header and named kit Dismiss button never scroll.
The body reuses `modal_scroll_body`, the existing read-only body keyboard
actions (arrows/Home/End/PageUp/PageDown), a retained focus/scroll owner and
the system scrollbar. Opening focuses the body; Escape and Dismiss restore
the initiating focus. The occluding help layer consumes wheel input without
moving background panes or another overlay, including at scroll boundaries.
Shortcut names, title and remap explanation follow EN/JA; raw command IDs
and keybinding syntax are unchanged.

The existing CloseTab command remains the single Cmd-W binding. Its handler
is enabled on document-free Home and delegates to guarded host closure only
when there is no tab/document to close; no parallel unguarded close route is
introduced.

## Amendment (2026-10-14): palette accessibility activation (#1513)

The existing `ListBoxOption` result row owns an explicit AccessKit `Click`
handler. Native assistive activation, pointer MouseDown and Input Enter share
the same action identity and enabled gate. MouseDown remains the pointer entry
because the dismiss scrim can unmount the palette before mouse-up; no pointer
`on_click` is added. Disabled results remain selectable and execution-inert.
Theme and language results use their existing settings-backed dispatch paths.
The `palette_rows_ax_action` regression inspects real AccessKit nodes and sends
AppKit's standard Press action through the production adapter, not a callback
recorder. Selection, heading ownership and selected-child reveal remain unchanged.

## Amendment (2026-10-15): Help shortcut glyphs (#1563)

Help → Keyboard Shortcuts remains a dense read-only info panel. The reference is
the existing native menu and command palette shortcut notation, not a new layout.
`shortcut_listing` formats each current `effective_keystroke` with the existing
`display_keystroke`; raw GPUI tokens never become the Help caption. On macOS this
shares pinned `Kbd::format` modifier order and named keys; other platforms retain
the existing text notation. EN/JA command labels retain their translation owner.
Each whitespace-separated chord stroke is formatted independently and joined
with one space on both platform paths. The non-macOS text projection is a pure
helper, also compiled into macOS unit tests, so chord and minus/named-key
coverage does not depend on a Linux/Windows runner. It appends into one output
buffer rather than allocating a vector of modifier strings for each stroke.

Startup custom bindings and unassigned commands keep their existing semantics.
The panel's scroll, focus, Escape/dismiss and return-focus behavior are unchanged;
no interactive control, settings writer, Git read/write or log line is added.
Hover/pressed/selected/loading/error states are not applicable to these static
captions. Native `help_shortcuts_glyphs` covers EN/JA, 100%/167%, Apple Light/Dark
and 940×660/1392×883, including startup remapping, named keys, unassignment,
palette agreement and remapped action dispatch. Real-window EN/JA captures remain
separate Tier B evidence, not native VoiceOver claims.
