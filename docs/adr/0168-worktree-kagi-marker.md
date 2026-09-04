# ADR-0168: Kagi-created worktree marker + bulk-prune scoping

- Status: Accepted
- Date: 2026-09-04
- Issue: #372 item 1 (follow-up from #340 / PR #371)
- Touches: `crates/kagi-git/src/ops/worktree_paths.rs`,
  `crates/kagi-git/src/ops/worktree.rs`,
  `crates/kagi-git/src/ops/worktree_lifecycle.rs`

## Context

Kagi's bulk worktree **prune** (`plan_prune_worktrees` / `execute_prune_worktrees`,
ADR-0340) removes the admin entry of any registered worktree whose working
directory is gone. It could not tell a worktree kagi created from one the user
set up by hand with `git worktree add`. A bulk op that sweeps up hand-added
worktrees violates the safety-first contract: kagi must never touch state the
user built outside kagi.

We need a durable, backend-auditable way to mark kagi's own worktrees so bulk
operations can be scoped to them.

## Decision

When kagi creates a linked worktree (`execute_create_worktree` /
`execute_open_worktree_for_branch`), it writes an empty marker file
`.kagi-created` into that worktree's git admin directory
(`$GIT_DIR/worktrees/<name>/`). `prunable_worktrees` — the single target
selector shared by both the prune plan and execute — skips any worktree that
does not carry the marker.

### §5 open question — writing into git's admin dir

Resolved: **acceptable.** The per-worktree admin dir is git's private metadata
namespace, and dropping a tool-owned marker there is precedented — Claude Code
added exactly this check in v2.1.246. The file is empty, ignored by git (git
never reads unknown files under `worktrees/<name>/`), and survives the one state
that matters for prune: when the working directory is deleted, the admin dir
(and the marker) remain, so the marker is readable exactly when the scoping
decision is made.

### Safety properties

- **Fail-safe default.** An unmarked worktree is treated as hand-added and left
  alone. If the marker write ever fails, the worst case is kagi declining to
  bulk-prune its own worktree — never wrongly deleting a user's. The write is
  therefore best-effort and never undoes an already-created worktree.
- **Bulk vs. explicit.** Only the *bulk* prune is scoped. Explicitly removing
  one specific worktree (`plan_remove_worktree` / `execute_remove_worktree`)
  still works on unmarked, hand-added worktrees — the user chose that target.
- **No destructive change.** Prune only drops stale admin entries whose workdir
  is already gone; no working tree is deleted. The marker narrows what prune
  touches; it never widens it.

## Consequences

- Worktrees created by kagi *before* this change have no marker, so bulk prune
  will now skip them (they read as hand-added). This is the safe direction —
  the user can still remove each explicitly. No migration is performed.
- `worktree_paths.rs` gains three small helpers (`worktree_admin_dir`,
  `mark_kagi_created`, `is_kagi_created`) alongside the existing containment
  helpers, keeping worktree admin-path logic in one module.

## Remaining (issue #372, out of scope here)

- **Item 2** — free-text lock-reason input modal (GUI; needs InputState wiring).
- **Item 3** — auto-lock worktrees with a running embedded-terminal process
  (uninvestigated; depends on gpui-terminal process-group handling, ADR-0035).
- **Item 4** — repo-wide prune/repair entry point reachable when no linked
  worktrees exist (GUI; toolbar / command palette / main-row menu).
