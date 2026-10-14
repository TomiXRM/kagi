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

## Amendment — 2026-10-15: New-worktree initial branch (#1564)

The branch menu's **Create worktree here** shares the commit menu's existing
new-branch form: the selected commit remains the start point, **New branch name**
(EN/JA, including the Input's accessible name) starts empty with its `branch-name`
placeholder, and the suggested path ends in
`new-branch`. It must not supply the existing source branch to a plan that
requires a new branch. **Open worktree from branch** retains its existing-branch
prefill and attachment semantics.

State contract for the existing input card (geometry/density unchanged):
- Empty branch: no inline required-name error; the placeholder requests input.
  The backend's blocker and disabled Create remain intact.
  Return on an empty/whitespace-only new-name field is a no-op: the form stays
  open, with no refusal receipt, toast or footer change. Existing-branch mode
  and nonempty invalid input retain their ordinary refusal behavior.
- Nonempty user input: ordinary localized validation, including collisions,
  remains visible. A valid new name enables Create through the same live plan.
- Untouched path follows the typed branch; manually chosen paths retain their
  existing ownership and validation.
- Cancel/open/edit remain read-only. Confirmation, preflight, execution,
  verification, receipts, locking and removal retain their existing safety path.

The shared pinned Input and modal Primary/Cancel buttons are reused; no new
control, validation owner or Git access during rendering is introduced.
