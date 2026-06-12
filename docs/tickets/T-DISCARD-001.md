# T-DISCARD-001: Discard backend — plan/backup/execute/verify + oplog

- Status: todo
- 関連: ADR-0046、lane W17-DISCARD

## スコープ

- `src/git/ops.rs` に `plan_discard(repo, paths) -> OperationPlan`(`destructive: true`)と
  `execute_discard(repo, &plan)`:
  1. backup: 各対象 path の WT 内容を `repo.blob()` で ODB へ(失敗 1 件で全体中止)
  2. `checkout_index`(path 指定 + force)で WT を index に戻す。**index/refs は不変**
  3. verify: status 再取得で対象が unstaged から消えたこと
- blockers: conflicted path / untracked path(対象に含まれていたら blocker、UI 側は事前除外)
- oplog: op="discard"、path→blob SHA リストを記録(復元手段)

## 完了条件

- [ ] unit/integration test(tests/discard_test.rs 新規): modify/deletion の discard、staged が不変、
      backup blob が ODB から読めること、conflicted/untracked blocker
- [ ] `cargo test` 全パス、own-code warning 0
