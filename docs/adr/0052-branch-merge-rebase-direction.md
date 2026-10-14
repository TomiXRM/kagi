# ADR-0052: Branch Merge/Rebase Direction Semantics

- Status: Accepted(2026-06-13、rebase 実装は MVP 外 — 本 ADR は意味論の確定まで)

## Decision

### 文言(必須)
- `Merge <target> into <current>` / `Rebase <current> onto <target>`。current branch 名を
  必ず両方の文言に含める。曖昧な `Merge` / `Rebase` 単独表記は禁止

### Merge(MVP: plan まで実装)
- 意味: 選択 branch(target)を **current branch へ** merge
- plan: in-memory merge(cherry-pick と同じ)で conflict を予測。**conflict 予測 = blocker**
  (repo 無傷で中止)。ff 可能かどうかを plan に表示(ff の場合は ref move のみと明示)
- 実行: ff なら ref move(checkout_tree → ref 順)、非 ff は merge commit 作成。
  dirty は warning + stash 提案(R6)

### Rebase(MVP 外 — 実装は別 wave)
- 意味: current branch を target の上へ rebase(SHA が変わる = history-rewriting、ADR-0023)
- pushed commit が range に含まれる場合 blocker(ADR-0040 案B と整合。案C 採用後に緩和検討)
- protected branch(main/master/develop/release 系)の rebase 禁止
- conflict 時は途中状態を作らない設計(in-memory で逐次適用し、失敗したら中止)を実装時に詳細化

## 改訂(2026-06-13): Merge の conflict は blocker をやめ、Conflict Mode へ遷移

Conflict Mode UI(ADR-0056 / W30-CONFLICT-UI)が入ったため、**merge の予測 conflict を
hard blocker にする方針を撤回**する(W31-MERGE-INTO-CONFLICT)。上記 §Merge の
「conflict 予測 = blocker」を本節で上書きする。

- plan(`plan_merge_branch`)は in-memory dry-run のまま。予測 conflict は **blocker ではなく
  warning + confirm**。戻り値に `MergeKind { FastForward, MergeCommit, Conflicts(files) }` を
  添えて呼び出し側へ「何が起きるか」を伝える(OperationPlan には欄を増やさない)。
- 実行(`execute_merge_into_conflict`)は git2 の本物 merge で marker + index stage 1/2/3 +
  `MERGE_HEAD` を残し、**commit はしない**。abort 用に `ORIG_HEAD` を pre-merge HEAD に書く。
  reload → `detect_conflict_session` で Conflict Mode に入場。
- abort は ADR-0056/0057 の既存 conflict abort(`cleanup_state` + ORIG_HEAD 復帰、force/
  reset --hard/clean 不使用)に委譲し、pre-merge の clean 状態へ戻す。
- 本改訂は **merge のみ**。cherry-pick / revert / stash-pop の同等化は別 wave。
- non-conflict(ff / merge-commit)の意味論は §Merge から変更なし。

## Amendment — 2026-10-12: bounded confirmation captions (#1343/#1344)

The plan heading retains both complete source and destination identities.
Its Rebase/Replay action names the source (`Rebase <source>` / JA
`<source> を rebase`), not the destination. Long action captions use a
zoom-scaled 160px middle-ellipsis region; the full action and directional
plan title remain available in the action tooltip/accessibility description.
This relaxes the original requirement to repeat both full names in every
visible action, not the requirement to disclose operation direction.
Replay retains its warning caption and existing two-stage approval gate.
The armed warning has no branch name and uses the ordinary, unellipsized
button so its complete safety wording remains visible in both languages.

## Amendment — 2026-10-14: Rebase plan scope (#1530)

The current branch remains attached and clean after a successful rebase, but
that alone does not disclose the operation. Plan time enumerates non-merge
commits reachable from HEAD but not the target, oldest first, with short OIDs
and subjects. The shared scrollable list shows the full candidate count and
supports Copy. EN/JA captions identify replay candidates, not pushes.
Git may skip equivalent patches or commits that become empty; this is a
candidate range, not a promise of a particular final SHA or replay count.
No render-time Git reads or plan-time repository writes are introduced.
Replay's ref-update script stays separate and is not relabeled as commits.
Planning runs off the UI thread through the existing session/visit-owned
planning lifecycle. The preview retains at most 100 subjects; a separate
typed scope retains the true total and presents a localized remaining count.
Copy formatting/joining happens only on activation. Linear ancestry with
no merge commits is already up to date, matching Git's no-op behavior.
The scope freezes the current ref/full OID and target operand/full OID.
CURRENT shows the original tip; AFTER identifies the target tip and marks
the result OID unknown. Mandatory preflight refuses target drift before
backup/write admission, and execution uses the approved target OID rather
than resolving the branch name again.


