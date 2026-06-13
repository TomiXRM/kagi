# ADR-0054: Branch Worktree Creation UX

- Status: Accepted(2026-06-13)

## Decision

- 「Open worktree from branch」=
  - branch が**既に別 worktree で checkout 済み** → その worktree path を案内(警告つき、新規作成しない)
  - それ以外 → 既存の create-worktree modal(path 入力 + live 検証 + plan)を branch 初期値つきで開く
- path 衝突は既存 plan_create_worktree の blocker をそのまま使用。作成後 Navigator の
  WORKTREES を refresh(既存 reload で充足)
- 「Create worktree from here」(commit menu 由来)と handler を共有(二重実装禁止)
