# ADR-0169 — Accountability UX: equivalent git command + `GIT_ADVICE=0`

Status: Accepted
Issue: #353 (parent #359)
Date: 2026-09-04

## Context

Kagi executes via **libgit2**, not the `git` CLI, so two things git users expect
are missing:

1. git's own `advice.*` output (≈40 items in `Documentation/config/advice.adoc`)
   never appears — the human-facing explanation only exists if kagi writes it.
2. There is no "command that was run" to show, because none is. But an
   **equivalent** command can be shown honestly — the Sublime Merge "Real Git"
   idea, adapted: *"this is equivalent to `<cmd>`"*, never *"runs `<cmd>`"*.

## Decision (this slice)

### 1. `OperationPlan.equivalent_command: Option<String>`

A new pure field on `OperationPlan` (`crates/kagi-domain/src/plan.rs`). It is
built as a plain string from plan data in the per-feature `ops/<feature>.rs`
builders — kagi-domain itself stays git2-free; it only carries the `Option`.

Emitted (`Some`) for the starter subset where the CLI mapping is genuinely
faithful — the destructive / network / history ops:

| Op | Equivalent | Where |
|---|---|---|
| push | `git push [-u] <remote> <branch>` | `ops/push.rs` |
| force-with-lease push | `git push --force-with-lease=<branch>:<lease> <remote> <branch>` | `ops/force_lease.rs` |
| branch delete | `git branch -d <name>` (kagi blocks unmerged deletes, so `-d`) | `ops/branch.rs` |
| checkout (branch) | `git checkout <branch>` | `ops/checkout.rs` |
| reset current → commit | `git reset --soft <sha>` (ref-only; never `--hard`) | `ops/reset.rs` |

Left `None` (honesty over coverage, issue §5):

- **discard** — libgit2's `checkout_index` diverges from `git checkout --` /
  `git restore` on eol normalization. A wrong equivalent is worse than none.
- every other plan kind (commit, merge, rebase, pull, stash, cherry-pick,
  revert, tag, worktree, …) — not yet vetted for faithfulness.

The plan modal renders one muted line via `Msg::PlanEquivalentTo`:
EN `"This is equivalent to \`{}\`"`, JA `"この操作は \`{}\` に相当します"`.
The wording is deliberately **"equivalent to" / "相当"**, never
**"runs" / "実行"** — asserted by a test — because kagi does not run the CLI.

### 2. `GIT_ADVICE=0` on subprocess `git` / `gh`

`crates/kagi-git/src/cli.rs` gained `git_command()` and `gh_command()` builders
that set `GIT_ADVICE=0` (alongside the existing non-interactive env). `run_git`
now goes through `git_command`; the ~12 `Command::new("gh")` sites across
`github.rs` / `github_merge.rs` / `ruleset.rs` now go through `gh_command()`.
This stops git's advice from doubling up with kagi's own UI guidance.

**Embedded terminal (`src/ui/terminal.rs`) deliberately left alone**: it runs
the user's interactive shell, not a kagi-driven git call. Forcing
`GIT_ADVICE=0` there would suppress advice in the user's own CLI, where it is
helpful (issue §5 open question — answered "no" for the interactive shell).

## Deferred (NOT in this slice)

- **advice.adoc 40-item catalog i18n** (`error.advice.*`) — the inventory of
  which advice items map to kagi situations, and their EN/JA strings.
- **Blocker wording rewrite** ("forbidden" → "next action") across all
  `PlanNote` blockers, plus optional action buttons.
- **Extending `equivalent_command` to the remaining plan kinds** — each needs a
  per-op faithfulness review before it can honestly emit.

These overlap the JP-wording work in **#376** and are tracked there / under the
parent #359.
