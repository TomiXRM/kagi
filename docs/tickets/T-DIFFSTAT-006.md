# T-DIFFSTAT-006: selected row / compact mode / tooltip の表示を調整する

- Status: todo
- 依存: T-DIFFSTAT-005

## スコープ

- selected row(accent 背景)でも数値・bar が読めること
- KAGI_COMPACT で行高が低くても破綻しないこと
- tooltip: `4 additions, 3 deletions`(gpui-component Tooltip、`.id` 必須)

## 完了条件

- [ ] 上記 3 点を PM スクリーンショットで確認できる状態

## 実装メモ (done)

- Status: done
- selected row: 数値・bar とも theme semantic color(accent 背景でも change_added/deleted は視認可)。
- compact mode(KAGI_COMPACT): bar は text_xs + 固定高 9px segment で行高に追従、破綻なし。path は min_w(0)+truncate。
- tooltip: gpui-component `Tooltip`(`.id` 必須 → `("diffstat-unit", id_seed)` で付与)。文言 `"N additions, M deletions"`(変更0は "No line changes")。
- PM スクリーンショット確認待ち(GUI 起動は PM 側)。
