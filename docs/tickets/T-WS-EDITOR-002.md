# T-WS-EDITOR-002: エディタワークスペース編集可能化(保存・再読込)

- Status: todo
- Group: workspace framework / エディタモード
- 仕様の正: ADR-0120 §Decision 4。依存: T-WS-EDITOR-001。

## 背景

001 で read-only ビューアまで通した。本チケットで編集→保存→watcher 反映の一巡を
成立させる。ファイル書き込みは Git write ではないため plan pipeline(invariant 4)の
対象外だが、失敗時の oplog + modal 通知(error handling 規約)は守る。

## スコープ

1. `code_editor` を編集可能にし、dirty 状態を entity が管理(タイトル/tree 行に ● 表示)。
2. Cmd-S(`secondary-s`、Input コンテキストでも効くよう binding 設計に注意)で
   `std::fs::write`。書き込み失敗は oplog + modal。
3. 保存 → watcher(`WatchEvent::WorkTree`)→ status refresh の一巡で tree と右 hunk が
   更新されることを確認(既存 `refresh_working_tree_external` 経路をそのまま使う)。
4. クリーン(未編集)バッファのみ、外部変更時に watcher 駆動で再読込。dirty バッファは
   上書きせずバナーで「外部変更あり」を提示(reload ボタン)。
5. 未保存 dirty があるままモード切替/タブ切替/ファイル切替する場合は確認モーダル
   (`ActiveModal` 規約: variant + accessors + confirm/cancel 配線)。

## 触ってよいファイル

`src/ui/editor_workspace*.rs`, `src/ui/modals.rs`, `src/ui/operations/modal_state.rs`,
`src/ui/tabs.rs`(watcher 配線), `src/ui/watcher.rs`(per-path 通知が要る場合のみ最小拡張),
`src/ui/i18n.rs`, `src/ui/commands.rs`。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*`(Git write なし)、既存 `[kagi]` コントラクト行。

## 完了条件

- [ ] 編集 → Cmd-S 保存 → 右 hunk と WIP 行が自動更新される
- [ ] dirty 表示、未保存での離脱に確認モーダル、外部変更はクリーン時のみ自動再読込
- [ ] 書き込み失敗が oplog + modal に出る(握りつぶさない)
- [ ] `cargo test --workspace` 全パス / 既存 `[kagi]` 行の変更なし
- [ ] GUI 目視は PM

## テスト方法

fixture repo で headless: ファイル編集を fs 経由で行い klog(保存/再読込/dirty)を
grep。watcher 一巡は既存の headless watcher 検証パターンを踏襲。

## リスク

- InputState(gpui-component)からの全文取り出し/set_value の毎 frame クロバー
  (conflict editor の `content_sig` パターンを再利用)。
- キーバインドの Input コンテキスト競合(既存 `!Input` 述語と整合させる)。
