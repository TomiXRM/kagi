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

