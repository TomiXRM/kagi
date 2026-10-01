# ADR-0213 — `nonconcurrent` worktree run mode

Status: Accepted
Issue: #859 (Refs #342, parent #359)
Date: 2026-10-01
Related: ADR-0171 (per-worktree ports + `KAGI_*`), ADR-0208 (terminal shell
ownership and exit observation, #772)

## Context

Per-worktree port blocks (ADR-0171) let worktrees run side by side. Some
projects cannot: one shared database, an external service whose callback URL is
fixed. For them, parallel worktrees are a footgun, and they need a way to give
up on parallelism deliberately (#342 §4, ADR-0171 Follow-up 2).

Kagi is not a process manager (#342 §7). The only thing it starts in a
worktree is its embedded terminal's shell.

## Decision

1. **What "running" means.** A worktree runs while Kagi holds a terminal shell
   for it whose exit it has not observed (`ShellProcess.exit` is `None`,
   ADR-0208). Processes started some other way are not seen.
2. **Setting.** `worktree_run_mode` in settings.json, a flat string:
   `"concurrent"` (default) or `"nonconcurrent"`; anything else reads as
   `concurrent`, so a typo never blocks a terminal. Typed accessor
   `Settings::worktree_run_mode()`. It is not stored in `.kagi/worktree.toml`.
3. **Scope.** In `nonconcurrent` mode, among the worktrees of one repository
   (same common git directory), one at a time may have a running shell. A
   second shell in the *same* worktree is not a second worktree running and is
   not refused; other repositories are unaffected.
4. **UX: block, with the reason.** Starting a terminal in another worktree of
   that repository spawns nothing. The terminal pane shows the reason, as do the
   footer and a toast (EN/JA, naming the running worktree), and
   `[kagi] terminal: nonconcurrent blocked <path> (running in <path>)` is logged.
   It is not a write, so there is no modal and no oplog entry. Once the running
   shell exits (observed by the background wait), opening the terminal again
   starts it.
5. **Decision is pure.** `kagi_domain::worktree_run_mode::blocking_worktree`
   decides from the mode, the target repository and worktree, and the live
   shells; the UI only gathers those inputs (`repository_of` in `kagi-git`
   reads a worktree's common directory).

## Not decided

- **One `KAGI_PORT` for every worktree of a nonconcurrent repository.** A fixed
  callback URL suggests every worktree should receive the same port, since only
  one runs at a time. Not adopted here: worktrees keep their own blocks
  (ADR-0171). Revisit if users of the mode ask for it.
- **Warn instead of block.** Blocking is the escape hatch's point; a warn-only
  mode can be a third value later.

## Consequences

- A dev server started outside Kagi's terminal (another terminal app, a
  launchd job) does not count and does not block.
- A shell whose exit could not be observed (`ShellExit::Unknown`) still counts
  as finished only once the wait reports; until then it keeps blocking, which
  errs on the side the mode exists for.
