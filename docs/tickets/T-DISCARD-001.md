# T-DISCARD-001: Discard backend — plan/backup/execute/verify + oplog

- Status: done
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

- [x] unit/integration test(tests/discard_test.rs 新規): modify/deletion の discard、staged が不変、
      backup blob が ODB から読めること、conflicted/untracked blocker
- [x] `cargo test` 全パス、own-code warning 0

## 実装メモ

- `src/git/ops.rs` に追加:
  - `plan_discard(repo, &[String]) -> OperationPlan`(`destructive: true`)。blockers =
    空選択 / conflicted / untracked / unstaged に存在しない path。untracked が tree に
    あれば warning「Untracked files are not deleted by kagi」も付与。
  - `execute_discard(repo, &plan, &[String]) -> DiscardOutcome`。順序は ADR 厳守:
    (0) plan.blockers があれば即中止 + preflight_check →
    (1) **backup** 各 path の現 WT 内容を `repo.blob()` で ODB へ。read 失敗で全体中止
        (unstaged deletion は WT 不在なので空 blob を記録し復元ハンドルを統一)→
    (2) `checkout_index(None, force + update_index(false) + disable_pathspec_match(true) + path 指定)`。
        **index は `update_index(false)` で不変**、refs も触らない →
    (3) verify: status 再取得で対象が unstaged から消えたことを確認(残れば Err)。
  - `DiscardBackup{path, blob}` / `DiscardOutcome{backups}` を返し、`oplog_summary()` で
    `"discarded N file(s); backup: path=<sha>, ..."` を生成。これを oplog の after.dirty に
    記録 = 復元ハンドル(`git cat-file -p <sha>`)。oplog スキーマは固定なので
    既存 StateSummary 文字列に path→blob を載せる方式を採用(新フィールド追加なし)。
- `src/git/mod.rs` に `plan_discard/execute_discard/DiscardBackup/DiscardOutcome` を re-export。
- tests/discard_test.rs(新規・7 tests, 全 green): modify discard / unstaged deletion discard /
  staged 不変(`:path` = STAGED のまま、backup は WT 内容)/ untracked blocker(WT 不変)/
  conflicted blocker / 空選択 blocker / 複数ファイル 1 オペ + summary に全 path。
- `cargo test` 全 24 suite green、own-code warning 0(`block v0.1.6` の将来非互換 warning のみ、依存側)。
