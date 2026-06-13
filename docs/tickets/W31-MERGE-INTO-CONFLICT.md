# W31-MERGE-INTO-CONFLICT: merge into a conflicting state instead of blocking

- Status: done / 担当: Opus lane
- 依存: W30-CONFLICT-UI(Conflict Mode UI + detect_conflict_session)、
  ADR-0052(改訂 2026-06-13)、ADR-0056(Conflict Mode 状態機械)
- スコープ: **merge のみ**(cherry-pick / revert / stash-pop は別 follow-up)

## 問題

予測 conflict を hard blocker にしていたため Execute ボタンが出ず、Conflict Mode UI が
あるのに詰んでいた(`plan_merge_branch` が blocker を push、`execute_merge_branch` が
"would produce conflicts. Re-plan" でエラー)。

## 望ましい挙動

warn + confirm(「N 件 conflict する — 続行?」)→ conflict marker を残す **本物の merge** を
実行 → Conflict Mode に遷移。abort は既存の conflict abort で pre-merge 状態へ復帰。

## 実装(完了 / 2026-06-13)

### src/git/ops.rs
- `pub enum MergeKind { FastForward, MergeCommit, Conflicts(Vec<String>) }` を追加。
- `plan_merge_branch` の戻り値を `Result<(OperationPlan, MergeKind), GitError>` に変更
  (OperationPlan には欄を増やさず、シグナルは MergeKind のみで運ぶ)。予測 conflict 時:
  blocker を push せず空のまま、conflict ファイルを列挙する WARNING を追加、
  `predicted.dirty = "N conflicted file(s) (resolve in Conflict Mode)"`、kind=Conflicts(files)。
  既存の本物の blocker(detached/unborn HEAD・already-contains・nothing-to-merge・
  pre-existing conflicted)は維持。
- `pub fn execute_merge_into_conflict(repo, target) -> Result<Vec<String>, GitError>` を追加。
  git2 の本物 merge(`find_annotated_commit` → `repo.merge(&[&annotated], None, safe checkout)`)で
  working tree に marker・index に stage 1/2/3・`.git/MERGE_HEAD` を作る。commit はしない。
  abort 用に `ORIG_HEAD` を pre-merge HEAD へ書く(git2::merge は書かないため)。
  conflict path を返す。force/reset --hard/clean は不使用。
- 既存 `execute_merge_branch`(ff + merge-commit)は非 conflict 用に維持。

### src/ui/mod.rs + i18n
- `MergePlanModal` に `kind: MergeKind` を保持。
- modal render(`render_merge_modal`)で Conflicts 時は confirm ラベルを
  `Msg::MergeAndResolveConflicts`(en: "Merge and resolve conflicts" / ja: "マージして衝突を解決")に、
  先頭に `Msg::MergeConflictWarning` の prominent warning を追加。非 conflict は従来どおり。
- `merge_blocking` を kind で分岐。Conflicts → `execute_merge_into_conflict` → `app.reload()`
  (reload が conflict 検出 guard をリセットし `detect_conflict_mode()` を再実行 → Conflict Mode 入場)。
  FastForward/MergeCommit → `execute_merge_branch`。oplog はどちらも記録。

### conflicts.rs / resolution.rs / conflict_view.rs
- 変更なし(API を使うのみ)。abort は既存 `plan_conflict_abort` / `execute_conflict_abort`。

## テスト(tests/branch_menu_ops_test.rs)
- ff / merge-commit plan に `kind` assert を追加。
- 旧 `merge_plan_conflict_is_blocker_*` を `merge_plan_conflict_is_warning_not_blocker_*` に置換:
  blockers 空 + kind==Conflicts(same.txt) + warning にファイル名 + predicted.dirty に "conflicted"
  + working tree 不変。
- 新規 `execute_merge_into_conflict_then_abort_restores_pre_merge_state`:
  実行後 MERGE_HEAD 存在 + index conflict + Merge session 検出 + ORIG_HEAD==pre-merge HEAD、
  abort 後 HEAD 復帰 + MERGE_HEAD 消去 + session なし + same.txt が "main\n" に戻る。
- `cargo test --workspace` green(36 suites ok / 0 failed)。own-code warning 0。

## 戻り値の形
`plan_merge_branch -> Result<(OperationPlan, MergeKind), GitError>`(tuple)。
`execute_merge_into_conflict -> Result<Vec<String>, GitError>`(conflict paths)。
