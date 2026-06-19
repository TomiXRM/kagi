# ADR-0103: Worktree-aware graph — multi-worktree WIP rows + HEAD markers

- Status: Accepted
- Date: 2026-06-19
- Amends: ADR-0088 (stash graph rows)

## Context

A repository's commit DAG is shared across all of its worktrees — a commit
exists in every worktree's history. What differs per worktree is the *checked-out
branch* and the *uncommitted working-tree state*. Until now kagi rendered a single
WIP row, hardcoded to lane 0, reflecting only the worktree it currently had open;
branches checked out in linked worktrees were indistinguishable from any other
branch in the graph.

A survey of GitButler / Fork / GitKraken / Tower / lazygit showed that no client
renders all *real* worktrees' working state simultaneously — they switch one
perspective at a time (GitButler's simultaneous lanes only work because it abandons
worktree isolation). Showing every worktree's HEAD and uncommitted state together
in one shared graph is therefore a differentiator, and a natural fit for kagi's
existing per-row WIP-diffstat machinery.

## Decision

The single shared commit graph stays the only graph (no per-worktree graph, no
tab-per-worktree). Two worktree-aware overlays are added on top of it:

1. **Per-worktree WIP rows.** `collect_worktrees` (`src/git/snapshot.rs`) now runs
   working-tree status in each worktree's own directory and records pending-change
   counts in the new pure `WorktreeWip { staged, unstaged, untracked }` on
   `kagi_domain::refs::Worktree`. The commit list draws one WIP row per *dirty*
   worktree (`render_wip_row`), each tinted with that worktree's lane colour
   (`theme().lane_color(idx)`) so the rows are distinguishable at a glance. The
   currently-open worktree's row stays interactive (click → commit panel) and
   carries the live `+/-` diffstat; linked-worktree rows are informational
   (count only, `i18n::wip_row_other`).

2. **Multi-HEAD markers.** A branch tip checked out in a worktree other than the
   current HEAD gets a 🌳 glyph in its badge (`build_badge_map`), matching the
   🌳 on each WIP-row chip.

The open repo's WIP row is driven by the *live* working-tree status, not by
matching a worktree's `is_current` flag, so click-to-commit and the `+/-`
diffstat keep working even when path canonicalization can't match the open repo.
`is_current` itself is computed on canonicalized paths (`std::fs::canonicalize`)
so symlinked/linked worktrees are still recognised as current.

`WorktreeWip` lives in `kagi-domain` (pure data, unit-tested there); the git2
status read that populates it stays in `src/git/`. No `git2` enters `src/ui/`.

`build_tab_view` emits a `klog!("worktrees: {n} total, {m} dirty")` contract line
for headless coverage.

## Consequences

- Every worktree's uncommitted state is visible at once, colour-coded, without
  leaving the current repo/tab.
- Status is read for each worktree on snapshot. Worktrees are few, and each path
  was already opened to resolve its branch name, so the added cost is bounded.
- A worktree opened as a tab is marked distinctly: `RepoInfo.is_worktree`
  (`repo.is_worktree()`) flows onto `RepoTab.is_worktree`, and the tab strip
  renders worktree tabs with a 🌳 marker plus a colour accent/wash. The tab
  colour is the **same lane colour as that worktree's WIP row** — `apply_tab_view`
  records the worktree's rank (`RepoTab.wt_color_idx`) and the strip paints
  `lane_color(rank)`, so a worktree reads with one consistent colour across its
  graph WIP row and its tab. Remote tabs keep their ☁ marker.
- The open repo's WIP row click opens the commit panel (stage/unstage). A linked
  worktree's WIP row click switches the open repo to that worktree (via
  `open_repository`), after which its row becomes the commit-panel one. This
  avoids the GitLens #5311 "visible-but-inert" trap: every WIP row is actionable,
  and write actions always target the repo kagi actually has open — never a
  silently-different worktree HEAD.
- Tab dedup compares canonicalized paths on both sides, so switching between the
  main repo and a worktree (or opening one from the other's WIP row) never
  spawns a duplicate tab for the same repository — even when an existing tab was
  created from a non-canonical path (CLI/session `/tmp` vs `/private/tmp`).
- Per-badge colour for the 🌳 markers (matching each WIP row's colour) is left
  for later; today the colour distinction lives in the WIP rows (coloured chip,
  left colour stripe, and a colour wash across the row).
- 🌳 relies on the platform emoji fallback (Inter has no such glyph). If a build
  target lacks colour-emoji fallback, swap for a monochrome worktree glyph.
