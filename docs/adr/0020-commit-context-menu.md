# ADR-0020: Commit Context Menu Architecture

- Status: Accepted / Date: 2026-06-12

## Decision

- **右クリック捕捉**: commit row(uniform_list の行)に `on_mouse_down(MouseButton::Right)`。
  graph node 単独の hit 判定はしない(行全体 = node を含むため要件を満たす)
- 右クリックで**対象行を選択状態にする**(既存 `select()` はトグルなので jump と同じ冪等ガードを使う)
- **menu は自前 overlay**(モーダル群と同方式)。gpui-component の `popup_menu` は
  PopupMenuExt が Focusable な親 view を要求し KagiApp の構造と合わないため使わない。
  `KagiApp.commit_menu: Option<CommitMenuState { row_index, position: Point<Pixels> }>` を描画
- **dismiss**: 全画面透明レイヤへの click-away / Escape(既存 CloseMainDiff と同様の action)/
  項目実行時。menu 表示中は下のリストへのイベントを通さない
- **位置**: クリック座標に anchor。画面端では上/左に flip(viewport との比較で clamp)
- **menu item model**(ADR-0021 の純関数が生成):
  ```rust
  struct MenuGroup { title: Option<&'static str>, items: Vec<MenuItem> }
  struct MenuItem { action: CommitAction, label: SharedString,
                    state: ItemState /* Enabled | Disabled(reason) | Hidden */,
                    dangerous: bool }
  ```
- ヘッダ行に `<short SHA> <title(chars 切り詰め)>`。Dangerous グループは赤系 + ⚠
- disabled 項目は薄色 + hover tooltip で理由(sidebar の name_tooltip と同方式)

## Consequences

- モーダルと同じ overlay z-order 管理に menu が加わる(menu < modal)
- 右クリック→選択変更は diff/inspector の更新を伴う(既存 select の副作用に乗る)
- headless 検証: `KAGI_CONTEXT_MENU=<row>` で menu model をログ出力
  (`[kagi] context-menu: row=N items=...enabled/disabled(理由)`)し、描画なしで出し分けを検証可能にする
