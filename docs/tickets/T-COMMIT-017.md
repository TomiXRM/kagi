# T-COMMIT-017: Split Commit 支援(file 単位)+ Commit to New Branch

- Status: todo / v0.2
- 依存: 既存 stage/unstage(T024 / T-UI-002)/ 既存 `plan_create_branch` / `execute_commit`
- 関連: lane W14-SPLIT / W14-NEWBRANCH、requirements-commit-suite.md

## 背景

コミット粒度の整理。(1) **Split Commit**: file 単位で一部だけ commit し残りは未 staged に残す UX 整理
(既存 stage/unstage で実質可能なものをガイド付きで)。(2) **Commit to New Branch**: 現在 branch ではなく
新 branch を作ってそこに commit する合成フロー。hunk 単位 split は later。

## スコープ

### Split Commit(file 単位)

- Commit Panel で「一部のファイルだけ stage → commit → 残りはそのまま」を **明示的にガイド**(既存 stage/unstage
  を使うだけ。新 backend 不要)。commit 後に残った unstaged を見せて「続けて次の commit」へ誘導。
- hunk 単位は **later**(本チケットでは file 単位のみ)。

### Commit to New Branch

- commit 直前に「新しい branch を作ってそこに commit」を選べる。実装は既存 plan の合成:
  **`plan_create_branch`(新 branch、現在 commit から)→ checkout → `execute_commit`** を 1 つの確認フローにまとめる。
- 新 backend を増やすより、既存 plan/execute を順に呼ぶ薄い合成関数(`commit_to_new_branch`)で実現。
  checkout は既存の安全 checkout(dirty 時の扱いは既存 plan_checkout の規約に従う)。detached からは later。

## 完了条件

- [ ] file 単位で部分 commit でき、残りが unstaged のまま残る(既存挙動 + ガイド)
- [ ] Commit to New Branch で新 branch 作成 → その branch に commit され、HEAD が新 branch を指す
- [ ] 合成フローが既存 plan 確認 + checklist を通る
- [ ] unit test: 部分 commit 後の残留 / new branch commit の round-trip、計 3+
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/ops.rs` or `src/git/staging.rs`(`commit_to_new_branch` 合成関数)/ `src/git/mod.rs`
- `src/ui/commit_panel.rs`(Split のガイド UI / New Branch 選択)
- `tests/commit_to_new_branch_test.rs`(新規)
- `docs/tickets/T-COMMIT-017.md`

## 触ってはいけないファイル

- `Cargo.toml` / 他チケットのファイル / undo・amend backend

## テスト方法

1. `cargo test`(exit code 確認)
2. fixture / tempdir のみ
3. checkout 合成は既存の安全 checkout 規約に従う(reset --hard / clean を足さない)

## リスク・規約

- New Branch 合成は既存 plan/execute を順に呼ぶ(独自の危険操作を作らない)
- checkout の dirty 扱いは既存 `plan_checkout` 規約を踏襲(staged を失わない)
- hunk 単位 split は本チケット対象外(later)
