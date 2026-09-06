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

`merge_pr` は `PrMergeReport { result, recording }`、`remote_pull` は
`RemotePullReport { result, recording }` を返す。`recording` は ADR-0175 と同じ
`backend::recording::Recording` で、append は `recording::finalize` 一箇所だけ
（`pub` へ引き上げた。writer を二重実装しない、`let _ = append_oplog` もしない）。
`Success + Recording::Failed` は「変更済み・記録失敗」であり、**再実行の根拠に
しない**。UI は成功 toast を出さず「changed but not recorded」として提示する。

### 結果不明を Failed にしない

**PR merge**: `gh` の非ゼロ終了は「未 merge」の証拠ではない。merge が成立して
から後段（`--delete-branch`、応答自体）が壊れることがある。非ゼロ終了時は
`gh pr view --json merged,mergedAt` で server 状態を再読し、server 側を正とする。

| 再読結果 | outcome |
|---|---|
| merged=true かつ `--delete-branch` 要求あり | `Partial`（削除は未確認） |
| merged=true | `Success` |
| merged=false（未 merge が確定） | `Failed` |
| 再読不能（gh 不在 / offline / 認証切れ） | `Unknown`（再送しない） |

**SSH remote pull**: child の停止を証明できない限り Failed にしない。

| 観測 | outcome |
|---|---|
| whole-command timeout、ssh の切断バナー（`Connection closed by` 等） | `Unknown`（remote の `git pull` が動作中かもしれない） |
| 非ゼロ終了で `CONFLICT` / `Automatic merge failed` | `Partial`（host の worktree/index は変化済み） |
| spawn 前失敗、明確な拒否応答（auth / host key / no tracking information） | `Failed` |

UI は生の exit ではなく**記録済み outcome** を見て提示する。`gh` が失敗しても
再読が merged なら「executed」であり、失敗として見せない。

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
| conflict save / continue / abort / skip | `conflict-save:*`、`<op>-continue` / `-abort` / `-skip` | `src/ui/operations/conflict.rs` |
| terminal 起動失敗 | `terminal-start` | `src/ui/mod.rs` |
| plan blocker の `Refused`（全 family） | 各 op 名 | `record_op` が persist |

`record_op_persist` の残存 caller はこの表が全数である。新しい writer を
ここへ足す前に、その family の実行境界で記録できないか先に確認する。

dir/file conflict（`conflict-dir-file:*`）はこの表に**含まれない**。writer は
既に `crates/kagi-git/src/ops/dir_file_conflict.rs` にあり、UI 側は表示専用の
`record_op` を呼んでいる。

## 配送

`Recording::Failed` の通知は `finish_op_on_main` の DropStale で消えてはならない。
`finish_op_on_main_settled` の settle 半分（stale-tab guard より前、DESIGN §4
「operation の終端化は常に先」）で owner repo 名付き app notice として配送する。
stash family の配送と同じ位置づけで、表示半分（`on_done`）だけが落ちる。

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
- `a_failed_gh_whose_reread_says_merged_is_not_recorded_as_a_failure` /
  `a_merged_pr_whose_branch_deletion_is_unproven_is_partial` /
  `a_failed_gh_that_cannot_be_re_read_is_unknown_not_failed` —
  fake `gh` が `pr merge` と `pr view` を別々に応答し、上の再読表を実証する。

`tests/remote_oplog_test.rs`（ローカル ssh transport fixture）:

- `remote_pull_records_success_and_failure_at_the_transport` — Success の
  receipt、拒否応答の Failed、実際に conflict を起こした pull の Partial。
- `a_remote_pull_that_loses_the_session_is_unknown_not_failed` — ssh の切断
  バナーで Unknown。

## 保証しないもの

再読は `gh` の 1 往復であり、その往復自体が壊れれば `Unknown` になる。
merged=true のときに `--delete-branch` が実際に成功したかは照会していない
（`Partial` に倒す）。branch 削除の確認を足すなら transport 往復が一回増える。
SSH 側の判定は ssh / git の出力文字列に依存する。marker に一致しない未知の
切断メッセージは `Failed` に落ちる。exit code だけで確定できる契約は無い。
