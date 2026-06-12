# ADR-0022: GitOperationPlan Integration for Context Menu

- Status: Accepted / Date: 2026-06-12

## Decision

- 新しい plan struct は**作らない**。既存 `OperationPlan` に
  `pub destructive: bool`(default false)を追加して全要求フィールドを満たす
  (対応表は requirements-context-menu.md §GitOperationPlan 統合)
- repository 状態を変更する menu 操作は**全て** plan → confirm → preflight → execute →
  verify → oplog の既存パイプラインに乗る。read-only 操作(Copy 系 / Show details /
  Compare 系)は plan 不要、oplog 記録もしない(Copy)
- **handler 一元化**: 
  ```rust
  pub enum CommitAction { ShowDetails, CopySha, CopyShortSha, CopyMessage,
      CreateBranchHere, CreateWorktreeHere, CherryPick, Revert,
      CheckoutCommit, CheckoutRef(String), CompareWithHead,
      CompareWithWorkingTree, ShowChangedFiles, ResetToCommit }
  impl KagiApp { pub fn dispatch_commit_action(&mut self, action: CommitAction,
      target: CommitId, window: &mut Window, cx: &mut Context<Self>) }
  ```
  Context Menu と Inspector Actions は**この dispatch のみ**を呼ぶ(直接 plan を組まない)
- **Revert の実行設計**(cherry-pick T015 と同パターンの in-memory 方式):
  `repo.revert_commit(commit, head, 0, None)` → in-memory index → conflict なら
  blockers に入れて**repo に触れない**。クリーンなら tree write → commit 作成 →
  `checkout_tree(旧 baseline)` → ref 移動(**ref-order 規則**: ref を先に動かさない)。
  merge commit は MVP では plan 生成前に disabled(ADR-0021)
- **Checkout commit(detached)** : `plan_checkout_commit(repo, id)` を新設。
  predicted に「detached HEAD になります」、warnings に branch 作成推奨、
  dirty なら安全 checkout 失敗の可能性を warning。実行は
  `checkout_tree(safe)` → `set_head_detached`(ref-order 規則準拠)
- 実行後の refresh は既存 `record_op` + `reload()` に乗る(toast = W3-NOTIFY が自動適用)

## Consequences

- OperationPlan に field 追加 → 既存 plan 構築箇所全部に `destructive: false` を
  追記する必要がある(コンパイルエラー駆動で漏れなし)
- dispatch 一元化により T-CM-062(二重実装禁止)が構造で保証される
