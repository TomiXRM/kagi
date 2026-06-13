# T-UNDOREDO-001: Operation Undo / Redo (after commit / merge), GitKraken-style

- Status: todo
- Group: 新機能 / operation history
- 仕様の正: ADR-0081. Reuses oplog (ADR-0074), ORIG_HEAD/reflog, soft undo_commit (ADR-0041).

## 背景 / 既存(調査済み)
- `Backend::plan_undo_commit` / `execute_undo_commit`(直近 commit を soft で undo、ADR-0041)。
- `src/git/conflicts.rs`:abort で ORIG_HEAD + reflog から復元(ref move の前例)。
- `src/git/oplog.rs`:`OpLogEntry`(before/after 要約、`OpOutcome`)。各 op の前後 HEAD を記録。
- 危険操作禁止(ADR-0023):`reset --hard` 不可。undo/redo は ref move(soft)+ reflog 保全のみ。

## スコープ(ADR-0081 の4層、UI から git を呼ばない、コミットは消さない)
1. **domain**:`OperationKind`、`HistoryEntry { kind, branch, before: CommitId, after: CommitId, summary }`、
   undo/redo スタック(push / undo / redo / cursor、新規 op で redo tail を truncate)。純粋・単体テスト可。
2. **git-backend(`Backend`)**:`plan_undo(entry)` / `plan_redo(entry)` が `OperationPlan` を作る
   (現 HEAD→target、保全内容、blockers)。`execute_undo` / `execute_redo` が branch ref を
   target SHA へ移動(libgit2 `set_target` + index/worktree を soft で整合、**hard 禁止**)、verify。
3. **app**:OperationHistory を app state に保持。ref-moving execute(commit/merge/cherry-pick/
   revert/amend/undo)成功時に `HistoryEntry` を push(before/after は pipeline/oplog が持つ snapshot から)。
4. **ui**:toolbar に **Undo**(既存を operation-history undo に一般化)+ **Redo** ボタン(cursor で enable/disable)。
   click で plan modal(preview)→ confirm で実行。view は git を直接呼ばない(ADR-0078)。

## MVP
- commit と merge の undo/redo(+ 他の ref-move op も乗れば対応)。soft(working tree 保全)。
- 対象外:stash/discard/checkout の undo、cross-session 永続、partial undo、branch 跨ぎ。

## 完了条件(受け入れ条件)
- [ ] commit 後に Undo で直前 commit が undo され(ref が parent へ、変更は working tree/index に保全)、Redo で戻る
- [ ] merge 後に Undo で merge 前 HEAD に戻り(merge commit は reflog に残る=消えない)、Redo で再適用
- [ ] Undo/Redo は plan modal で preview(HEAD before→after、保全内容、blockers)を出し、confirm するまで実行しない
- [ ] **コミットを破壊しない**(reflog/ODB から復元可能)。`reset --hard` を使わない
- [ ] toolbar に Undo + Redo ボタン、cursor 位置で enable/disable が正しい
- [ ] 新規 op を行うと redo スタックが truncate される
- [ ] stale entry(branch が after でない / target 到達不可)は preflight で検出し理由表示して skip
- [ ] domain の undo/redo スタックの単体テスト + backend の plan/execute_undo/redo の fixture integration test(commit undo→redo、merge undo→redo)
- [ ] `cargo test --workspace` 全パス + `grep -rE 'git2::|Repository::open' src/ui` = 0

## 規約
- UI に git 直書き禁止(Backend 経由)。`reset --hard`/clean/force 禁止(ADR-0023)。
- 文字列は i18n `Msg`。色は `theme()`。fixture/tempdir のみで検証。
- undo/redo も oplog に記録する。

## やってはいけないこと
hard reset で undo / コミット破棄 / drop 即実行(preview を飛ばす)/ UI から git 呼び出し /
dirty working tree の変更を黙って捨てる。

## Implementation memo
(担当 agent が完了時に追記)
