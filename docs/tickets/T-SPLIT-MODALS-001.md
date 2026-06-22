# T-SPLIT-MODALS-001: modal_renderers.rs をモーダル系列で分割

- Status: todo
- Group: god-file split (CLAUDE.md ≤800 LOC/file) — ADR-0116 Wave 3 フォローアップ
- 仕様の正: ADR-0116 Wave 3 / 先行 T-SPLIT-HELPERS-001

## スコープ

T-SPLIT-HELPERS-001 で共通オーバーレイ筐体（`modal_overlay`）は抽出済みだが
`src/ui/modal_renderers.rs` は依然 3117 LOC（≤800 目標を大きく超過）。1モーダル=1関数の
機能境界を保ったまま、モーダル系列ごとに sibling モジュールへ**逐語移動**する。

分割案（系列で grouping。実際の関数名は Read で確認して調整）:
- `modal_renderers_checkout.rs` — checkout / plan / create-branch / create-worktree 系
- `modal_renderers_remote.rs` — pull / push / set-upstream 系
- `modal_renderers_stash.rs` — stash push/apply/pop/drop 系
- `modal_renderers_history.rs` — amend / undo / cherry-pick / revert 系
- `modal_renderers_misc.rs` — smart-commit / auto-update / その他
- `modal_renderers.rs` 本体には `modal_overlay` 等の共通ヘルパと `pub(crate) use` 再エクスポートを残し、呼び出し元（render_overlay 等）を**触らずに**公開パス維持。

## 完了条件

- [ ] modal_renderers.rs 本体および各新ファイルが ≤800 LOC
- [ ] 1モーダル=1関数の境界維持、呼び出しパス不変（再エクスポート）
- [ ] 出力 DOM・イベントハンドラ・`[kagi]` 契約行・i18n(Msg) 不変（純粋移動）
- [ ] `cargo build` + `cargo test --workspace` green、`cargo fmt --check` clean、新規 clippy 警告なし
- [ ] **UI 目視検証 pending** を明記
- [ ] 実装メモを末尾に追記

## 規約

- 移動のみ。Entity 化や入力ロジック改変には踏み込まない
