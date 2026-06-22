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

## 実装メモ (T-SPLIT-MODALS-001 完了)

- Status: done（UI 目視検証 pending — GUI はサブエージェントで起動不可。primary/人間が要確認）
- 手法: 先行 T-SPLIT-HELPERS-001 と同じ「逐語移動 + 再エクスポート」。`render_overlay.rs` の
  `use crate::ui::modal_renderers::*;` は一切触らず、`modal_renderers.rs` 本体に
  `pub(crate) use super::modal_renderers_<series>::*;` を置いて公開パスを維持。
  関数本体は無改変（DOM / イベントハンドラ / `[kagi]` 契約行 / i18n(Msg) 不変）。

### 新構成（各 LOC, すべて ≤800）

| ファイル | LOC | 内容 |
|---|---|---|
| `modal_renderers.rs`（本体） | 458 | 共通ヘルパ `modal_overlay` / `render_plan_modal_card` / `render_input_plan_modal` を保持 + 各 sibling の再エクスポート |
| `modal_renderers_plan.rs` | 544 | `render_plan_modal_card` / `render_input_plan_modal` に委譲する薄いラッパ群: checkout / pull / undo / history / conflict_continue / pop / stash_drop / push / branch_plan / set_upstream / rename_branch / merge / tracking_checkout / switch_to_latest / delete_branch / revert |
| `modal_renderers_destructive.rs` | 461 | 履歴書換え・破壊系の二段確認カード: amend / discard |
| `modal_renderers_create.rs` | 475 | 名前入力 + ライブプラン + ESC/focus ラッパ: create_branch / create_worktree |
| `modal_renderers_stash.rs` | 411 | stash_push / stash_apply |
| `modal_renderers_commit.rs` | 513 | preview file-tree を持つ: cherry_pick / commit_plan |
| `modal_renderers_misc.rs` | 354 | smart_commit（consent/model-picker）/ update（auto-update 詳細） |

（分割前 `modal_renderers.rs` = 3117 LOC）

### 触れたファイル

- `src/ui/modal_renderers.rs`（本体をトリム + 再エクスポート追加）
- 新規 `src/ui/modal_renderers_{plan,destructive,create,stash,commit,misc}.rs`
- `src/ui/mod.rs`（6 件の `mod modal_renderers_*;` 宣言を追加）

`render_overlay.rs` 等の呼び出し元・`crates/` は無改変。

### 検証

- `cargo build` green / `cargo fmt --check` clean
- `cargo test --workspace` = 791 passed, 0 failed
- `cargo clippy --workspace` 新規警告なし（modal_renderers* に新規 warning 0。既存 debt 38 件は不変・未着手）
