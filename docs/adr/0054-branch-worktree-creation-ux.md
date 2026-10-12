# ADR-0054: Branch Worktree Creation UX

- Status: Accepted(2026-06-13)

## Decision

- 「Open worktree from branch」=
  - branch が**既に別 worktree で checkout 済み** → その worktree path を案内(警告つき、新規作成しない)
  - それ以外 → 既存の create-worktree modal(path 入力 + live 検証 + plan)を branch 初期値つきで開く
- path 衝突は既存 plan_create_worktree の blocker をそのまま使用。作成後 Navigator の
  WORKTREES を refresh(既存 reload で充足)
- 「Create worktree from here」(commit menu 由来)と handler を共有(二重実装禁止)

## Amendment — 2026-10-13: Create Branch warnings (#1385)

Create Branch displays all existing `plan.warnings` in the scrollable plan
body before approval, using the shared warning-row icon and theme warning
colour with localized EN/JA text and the existing #354 `Role::Note` convention.
Create Worktree uses the same warning list. Cancel/Create remain
outside that body. Warnings are advisory: only existing blockers, pending
plans or failed plans disable Create. This presentation change does not
fetch rulesets, modify `plan_*`, or alter checkout-after safety policy.
