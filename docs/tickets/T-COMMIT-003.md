# T-COMMIT-003: Commit Checklist — checklist module(純関数)+ block/warn 統合

- Status: todo
- 依存: ADR-0039 / 0043 / 既存 `plan_commit`
- 関連: lane W14-CHECK

## 背景

ADR-0043 の checklist ルールを純関数 module に切り出し、`plan_commit`(と将来 `plan_amend`)から呼ぶ。
本チケットは **module の骨格 + 既存ルール(staged 空 / message 空 / conflict 状態)を移管 or 呼び出し統合**まで。
個別の新ルールは T-COMMIT-004〜006 で追加。

## スコープ

- 新 module `src/git/checklist.rs`:
  ```rust
  pub struct ChecklistInput<'a> { /* repo 状態 + staged 情報 + message */ }
  pub struct ChecklistResult { pub blockers: Vec<String>, pub warnings: Vec<String> }
  pub fn run_checklist(input: &ChecklistInput) -> ChecklistResult;
  ```
- 既存 `plan_commit` の blocker/warn 判定を、可能な範囲で `run_checklist` 経由に寄せる(挙動は不変、回帰なし)。
- block/warn の分類は ADR-0039 に従う。純関数(UI / oplog / ネットワークを持たない)。

## 完了条件

- [ ] `checklist.rs` が追加され、`plan_commit` が同等の blocker/warn を返す(既存 commit テスト回帰なし)
- [ ] unit test: staged 空 / message 空 / conflict 状態 で blocker(最低各 1)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/checklist.rs`(新規)/ `src/git/staging.rs`(plan_commit を checklist 呼び出しに)/ `src/git/mod.rs`(re-export)
- `tests/checklist_test.rs`(新規)
- `docs/tickets/T-COMMIT-003.md`

## 触ってはいけないファイル

- `src/ui/*` / `src/main.rs`(UI は PM)/ 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(exit code 確認)
2. tempdir のみ
3. 既存 commit 系テストが回帰しないことを確認

## リスク・規約

- 既存 `plan_commit` の blocker 文言を変えると UI テストに影響しうる → 文言はできるだけ温存
- 文字列切り詰めは `chars()` ベース
