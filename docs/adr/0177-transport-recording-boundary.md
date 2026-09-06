# ADR-0177: transport mutations record at their execution boundary

- 状態: 採用（#501）
- 日付: 2026-09-07
- 関連: [#501](https://github.com/TomiXRM/kagi/issues/501)、
  [ADR-0149](0149-oplog-in-backend-run-and-schema.md)、
  [ADR-0175](0175-app-remove-boundary.md)、[ADR-0176](0176-app-stash-local-boundary.md)、
  [DESIGN §5.2/§5.3/§7](../rearch/app-layer/DESIGN.md)

## 決定

remote mutation（`github::merge_pr`、`remote::remote_pull`）は **自分の実行境界で
記録し終えてから返る**。UI の完了 callback は表示専用にする。

`merge_pr` は `PrMergeReport { result, recording }` を返す。`recording` は
ADR-0175 と同じ `backend::recording::Recording`（`finalize` を再利用、writer の
二重実装はしない）。`Success + Recording::Failed` は「GitHub 上ではマージ済み・
記録失敗」であり、**再実行の根拠にしない**。UI はその旨を app notice に出す。

`remote_pull` は sibling の `remote_stash_drop` と同じ形にし、`before` を受け取り
`{host}:{repo}` scope の entry を append してから結果を返す。

plan blocker による `Refused` は transport に到達しないので UI が記録者のまま。
`record_op` は `Refused` だけを persist する既存規約（ADR-0149）で足りるため、
`record_op_persist` は使わない。

## 問題

`finish_op_on_main` は `OpDisposition::DropStale` で `on_done` を丸ごと捨てる
（`src/ui/operations/mod.rs`）。PR merge は `on_done` の中でだけ
`record_op_persist` を呼んでいたため、**GitHub 側で実際にマージが成立していても
タブを切り替えただけで oplog に一件も残らない**経路があった。
remote pull はさらに悪く、UI 側が非 persist の `record_op` を呼ぶだけで
transport も記録しないため、**成功しても常に未記録**だった。

## 記録所有表（2026-09-07 時点）

| family / 入口 | 記録者 | 実行 | 備考 |
|---|---|---|---|
| `Backend::run` に入る全 operation | `run_recorded` の単一 finalize | mixed | ADR-0149。UI は表示のみ |
| worktree remove | `backend/remove.rs` boundary | async | ADR-0175 |
| local stash push/apply/pop/drop | `run_recorded` + stash evidence | async | ADR-0176 |
| history undo/redo | `run_history_move` | sync | `record_run_oplog` |
| branch cleanup | `execute_delete_merged_branches`。open 失敗は job 内で append | async | #519 |
| dir/file conflict の index 解決 | `ops/dir_file_conflict.rs` | sync | ops 層で append |
| **PR merge** | **`github::merge_pr`（本 ADR）** | async | 受領した試行は必ず記録 |
| **remote pull (SSH)** | **`remote::remote_pull`（本 ADR）** | async | 以前は未記録 |
| remote stash drop (SSH) | `remote::remote_stash_drop` | async | ADR-0097 |

### 例外 — まだ UI が writer の同期経路

以下は **すべて同期実行**で、`finish_op_on_main` を通らない。従って
stale completion で記録を失う経路が構造的に存在しない。所有の移管は
DESIGN §5.3（"Backend へ移す"）の残作業として据え置く。

| writer | op 名 | 場所 |
|---|---|---|
| `confirm_lock_worktree` | `lock-worktree` | `src/ui/operations/worktree.rs` |
| `confirm_unlock_worktree` | `unlock-worktree` | 同上 |
| `confirm_prune_worktrees` | `prune-worktrees` | 同上 |
| `confirm_repair_worktrees` | `repair-worktrees` | 同上 |
| conflict save / dir-file / continue / abort / skip | `conflict-save:*`、`conflict-dir-file:*`、`<op>-continue` / `-abort` / `-skip` | `src/ui/operations/conflict.rs` |
| terminal 起動失敗 | `terminal-start` | `src/ui/mod.rs` |
| plan blocker の `Refused`（全 family） | 各 op 名 | `record_op` が persist |

`record_op_persist` の残存 caller はこの表が全数である。新しい writer を
ここへ足す前に、その family の実行境界で記録できないか先に確認する。

## 検証

`tests/transport_recording_test.rs`:

- `pr_merge_records_before_returning_when_the_ui_completion_is_dropped` —
  fake `gh` を PATH に置いて成功/失敗の両方を実行し、UI の completion を
  **一度も呼ばずに** entry が append 済みであることを確認する。
  head SHA（`--match-head-commit` に束ねた復元材料）と worktree/actor も確認。
- `pr_merge_reports_recording_failure_without_hiding_the_merge` —
  oplog のファイル位置をディレクトリにして append を失敗させ（DESIGN の
  決定的 fixture）、`result` は `Ok`、`recording` は `Recording::Failed` に
  なることを確認する。
- `remote_pull_records_success_and_failure_at_the_transport` —
  `tests/remote_oplog_test.rs` と同じローカル ssh transport fixture。

## 保証しないもの

外部 remote の「結果不明」と確定 failure の区別（`gh` の timeout/切断）は
本 ADR の対象外。現状は `gh` の非ゼロ終了をすべて `Failed` として記録する。
transport 用の `Unknown` は remove/stash と同じ判定材料を持たないため、
別途 #505 / transport family の設計で決める。
