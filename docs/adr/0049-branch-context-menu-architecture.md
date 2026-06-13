# ADR-0049: Branch Context Menu Architecture

- Status: Accepted(2026-06-13)
- 関連: requirements-branch-context-menu.md、ADR-0020〜0026(commit context menu)

## Decision

- **既存 context_menu.rs の MenuGroup / MenuItem / overlay 機構を再利用**する(backdrop+card とも
  `.occlude()` 必須 — 既知の click-through バグ)。branch 専用の組み立ては新 module
  `src/ui/branch_menu.rs` に置く
- menu 構築は純粋関数 `branch_context_menu_items(ctx: &BranchMenuContext) -> Vec<MenuGroup>`
  (ADR-0050)。UI は sidebar.rs の branch 行に `on_mouse_down(Right)` を付け、対象 branch を
  selection state に反映してから anchor 位置で overlay を開く
- **dispatch は既存 handler を呼ぶだけ**(start_checkout / start_pull / start_push /
  open_create_branch_modal / open_delete_branch_modal / start_create_worktree / copy 系)。
  Context Menu 専用の git 実装を作らない(二重実装禁止)
- 新規操作(set upstream / rename / merge / non-current pull(ff-only ref update)/
  create tracking branch)は `src/git/ops.rs` に plan_*/execute_* を追加し、Header/Panel からも
  使える形で実装する
- folder/group 行は MVP では menu を出さない(no-op)
