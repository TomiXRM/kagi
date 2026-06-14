# T-UNDOREDO-001: Operation Undo / Redo (after commit / merge), GitKraken-style

- Status: done (PM accepted — GUI-verified 2026-06-14)
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

実装ブランチ: `rearch/undo-redo`（`re-architecture` ベース）。4 層で実装、UI から git 直書きなし
（`grep -rnE 'git2::|Repository::open' src/ui` = 0）。

### 1. domain — `crates/kagi-domain/src/history.rs`(+ `lib.rs` に `pub mod history;`)
- `OperationKind`(Commit/Merge/CherryPick/Revert/Amend/UndoCommit、`slug()` 付き)。
- `HistoryEntry { kind, branch: String, before: CommitId, after: CommitId, summary: String }`。
- `OperationHistory { entries, cursor }`：`record`(redo tail を truncate)、`can_undo/can_redo`、
  `peek_undo/peek_redo`、`undo()`/`redo()`(cursor を前後)。純粋・git2/gpui 非依存。
- 単体テスト 10 本（new/record/undo→redo round-trip/LIFO/truncate/peek 不変/全 undo→全 redo/slug）。

### 2. git-backend — `src/git/ops.rs`(+ `backend.rs` ラッパ、`mod.rs` 再公開)
- `plan_undo`/`plan_redo`(共通 `plan_history_move`)→ `OperationPlan`(現 HEAD→target、preview、
  blockers/warnings)。blocker: branch が current HEAD でない / branch が `from` にない(stale) /
  target が ODB に無い(stale) / conflict 中。dirty WT は **warning**(保全して続行)。
- `execute_undo`/`execute_redo`(共通 `execute_history_move`)：
  PREFLIGHT で branch=current・branch が `from`・target 到達可能を再確認 → stale は明示エラーで skip。
  **safe ref move**: `repo.reference(refs/heads/<branch>, to, true, msg)` でブランチ ref を移動 →
  `index.read_tree(to_tree)` + `index.write()` で index のみ target tree に整合(**working tree は一切触らない**)→
  HEAD==target を verify。`reset --hard`/clean/force は不使用。`HistoryMoveOutcome { branch, from, to }` を返す。
- undo=branch を after→before、redo=before→after。merge commit の undo も同じ ref move で安全
  (merge commit は reflog/ODB に残存、WT もユーザーの状態のまま)。

### 3. app — `src/ui/mod.rs`
- `KagiApp` に `operation_history: OperationHistory` と `history_modal: Option<HistoryPlanModal>`(両 constructor で初期化)。
- `record_history(kind, branch, before, after, summary)`(before==after / branch 空 は no-op)。
- ref-move 成功ハンドラで記録：commit / merge(clean 時のみ、conflict は before==after で no-op) /
  cherry-pick / revert は background 完了時に `head_branch_and_sha()` で before(spawn 前)・after(完了時)を取得して記録。
  amend は `outcome.old→new`、undo-commit は `outcome.undone→now_at` を記録。
  `head_branch_and_sha()` は `Backend` 経由(UI に git2 無し)。
- `open_history_undo_modal`/`open_history_redo_modal`/`open_history_modal`/`confirm_history`:
  plan→preflight→execute→cursor 前後(execute 成功後にのみ undo()/redo())→oplog 記録(`undo-<kind>`/`redo-<kind>`)→reload。

### 4. ui — toolbar / modal
- toolbar の Undo ボタンを operation-history undo に一般化(enable=`can_undo()`、click→`open_history_undo_modal`)、
  **Redo ボタンを追加**(`IconName::Redo2`、enable=`can_redo()`)。tooltip は `peek_undo/peek_redo` の summary。
- `HistoryPlanModal` + `render_history_modal`(`render_plan_modal_card` 再利用、confirm=`confirm_history`)を
  既存モーダルオーバーレイ群に組み込み。Enter ガードにも追加。
- i18n: `Msg::{Undo, Redo, NothingToUndo, NothingToRedo}`。`Undo`/`Redo` は ADR-0048 のドメイン語として
  両言語英語。旧 undo-commit の disabled 理由文字列(UndoDetached/UndoUnborn/UndoAhead0)は generalize で不要になり削除。

### tests
- domain: `crates/kagi-domain/src/history.rs` 内 10 本。
- integration: `tests/undo_redo_test.rs` 5 本 —
  `commit_undo_then_redo`(HEAD が parent→forward、commit は reflog/ODB に残存) /
  `merge_undo_then_redo`(merge commit を undo→pre-merge、merge commit は残存→redo で再適用) /
  `undo_preserves_working_tree_changes`(未コミット編集が保全) /
  `plan_undo_stale_entry_is_blocked`(branch 移動後は plan blocker + execute エラー) /
  `undo_redo_pipeline_via_domain_history`(domain OperationHistory + Backend を連結)。
- `cargo test --workspace`: 全パス(0 failures)。`grep -rnE 'git2::|Repository::open' src/ui` = 0。
  注: `tests/i18n_test.rs` は実 GUI バイナリを 4 つ並列起動する smoke test で、ウィンドウ生成競合により
  並列フル実行時に稀に flaky(単独/再実行では green)。本変更とは無関係。

### 不確実点 / レビュー要点
- **merge commit の undo の安全性**: ref move + `index.read_tree` のみで HEAD を 2-parent merge commit から
  pre-merge へ戻す。merge commit は reflog/ODB に残るので破壊なし。WT は触らないので merge で生成された
  作業ツリー変更は残る（index は pre-merge tree に整合）。integration test で検証済み。
- **working-tree 保全**: mixed 相当(ref+index のみ移動、WT 不変)。`git reset --hard`/clean は不使用。
  ただし「commit を undo」した場合、変更は index(staged)ではなく WT(unstaged)として現れる
  (`index.read_tree(target)` のため)。旧 `undo_commit`(ADR-0041、soft=staged 維持)とは差異あり。MVP では許容。
- in-session のみ(終了で消える)。reflog が durable backstop。

## PM acceptance (2026-06-14, GUI-verified with cliclick)
Drove the full cycle in the running app on a fixture repo:
- Drag-merged feature/two → main (confirmed). `history: record merge on 'main' b0bd2b2 → d3015d9`. Undo button enabled, Redo disabled.
- **Undo** → preview modal "Undo merge on 'main' — d3015d96 → b0bd2b28" (safe mixed-reset, "uncommitted changes preserved", recovery recipe shows the merge SHA). Confirmed → HEAD b0bd2b2; **merge commit d3015d9 still in ODB + reflog (not destroyed)**; Redo enabled.
- **Redo** → preview → confirmed → HEAD d3015d9 (merge restored, 2-parent).
- redo-2.svg added so the Redo toolbar icon renders (was missing from the asset allowlist).
- cargo test --workspace green (10 domain + 5 integration tests); src/ui git2-free.
