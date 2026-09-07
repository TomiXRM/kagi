# ADR-0185: Graph worktree navigation

- Status: Accepted
- Date: 2026-09-07
- Issue: #591
- Builds on: ADR-0025 (worktree UX), ADR-0174 (worktree HEAD identity),
  ADR-0182 (session identity and attachment)

## Context

The graph exposed `OpenWorktreeDir` only through a local branch badge's
right-click menu. The tree glyph had no action or path tooltip, main-worktree
branches deliberately had no glyph, and detached worktrees had no branch badge
at all. Clean detached worktrees therefore had no graph entry point.

## Decision

The graph's tree glyph means exactly "checked out in another worktree". It is
shown for linked and main worktrees alike; the current worktree retains the ✓
HEAD marker. The glyph is a dedicated click target with a localized path
tooltip. Its hitbox is a sibling of the branch name, so clicking the name keeps
the existing row jump and double-click checkout behavior.

`RefBadge` carries snapshot-derived worktree navigation metadata (registry
name, path, lock and main-worktree state). This is a view model, not another
state owner. Clicking the glyph delegates to `KagiApp::open_repository`, whose
canonical path and SessionId attachment checks switch to an existing tab rather
than creating a duplicate. The only new contract line is
`[kagi] worktree-open: <path>`.

A non-current detached worktree contributes a `Worktree` badge at its recorded
`Worktree.head`, labelled `🌲 detached <short-sha>`, whether clean or dirty. It
uses the existing worktree context menu. A detached main worktree gets the path
and repo-wide menu actions but not linked-only lock/remove actions.

The new badge priority is after HEAD and before branches/tags/remotes. It stays
inside the graph's two visible slots even when several refs share its commit;
existing ref kinds retain their relative order and the existing `+N` overflow
behavior.

## Consequences

- No new Git read, manager, persistent UI field or worktree-opening path is
  introduced; `build_badge_map`, `render_badges_column` and `open_repository`
  remain the owners.
- A detached worktree badge is navigation, not a ref: it is not copied as a
  branch, used as a merge drag source, checked out, or given a branch menu.
- GUI evidence must exercise the tree/name hitboxes separately, tab reuse, and
  clean detached navigation. The scenario is built for focused PM execution;
  this implementation does not run the GUI runner.
