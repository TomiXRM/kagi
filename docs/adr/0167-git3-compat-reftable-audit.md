# ADR-0167: Git 3.0 compat — `.git/` direct-read audit and `add -p` split UX

- Status: Accepted
- Issue: #357 (parent tracking #359)
- Date: 2026-09-04

## Context

Git 3.0 makes **reftable** the default ref backend for new repositories
(declared in 2.51). Under reftable, refs no longer live as loose files under
`.git/refs/` or in `packed-refs` — they move to `.git/reftable/`. Any code that
reads `refs/heads/*`, `refs/tags/*`, or `packed-refs` **directly from the
filesystem** breaks on a reftable repo.

Local git here is **2.50.1**, so the 2.52+ / 2.55+ surface (`git repo info`,
`git url-parse`, `hook.<event>.enabled`) is not testable. This PR is scoped to
the two items that are fully verifiable now: the direct-read **inventory**, and
the **`add -p` split UX** question. The rest is documented as deferred below.

## Decision — Part 1: `.git/` direct-read inventory

Every direct `.git/…` filesystem read in `crates/kagi-git` and `src/` was
enumerated (`grep` for `repo.path().join` / `git_dir.join` / `fs::read*` of git
paths). **All direct reads live in `crates/kagi-git/src/conflicts.rs`.** They
split cleanly into two safe categories — no real-ref reads exist.

### Classification rule

- **Pseudoref** — `HEAD` and the operation-in-progress heads (`ORIG_HEAD`,
  `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `REBASE_HEAD`). Per the git
  docs these are **special refs** that remain real files directly in
  `$GIT_DIR`, *not* in the ref backend, even under reftable
  (`gitrepository-layout`: "pseudorefs … are stored as files under `$GIT_DIR`";
  the reftable design excludes the HEAD family). **Safe to keep.**
- **Sequencer / merge state** — `MERGE_MSG`, `rebase-merge/*`,
  `rebase-apply/*`. Not refs at all; plain state files git itself writes under
  `$GIT_DIR` regardless of ref backend. **Safe to keep.**
- **Real ref** — anything reading `refs/heads/*`, `refs/tags/*`, or
  `packed-refs` from disk. These **break under reftable** and must move to a
  git2 API (`repo.find_reference` / `references` / `reference_names`).
  **None found.**

### Inventory (`crates/kagi-git/src/conflicts.rs`)

| Line | What it reads | Category | Action |
|---|---|---|---|
| 352 (`read_head_ref`) | `$GIT_DIR/{MERGE_HEAD,CHERRY_PICK_HEAD,REVERT_HEAD}` | pseudoref | keep |
| 372–374 (`read_rebase_progress`) | `rebase-merge/{msgnum,end}` | sequencer state | keep |
| 381–384 (`read_rebase_commit`) | `rebase-merge/{stopped-sha,orig-head}` | sequencer state | keep |
| 909 | `$GIT_DIR/MERGE_MSG` | merge state | keep |
| 1496 | `$GIT_DIR/MERGE_HEAD` | pseudoref | keep |
| 1506 | `$GIT_DIR/MERGE_MSG` | merge state | keep |
| 1678 | `$GIT_DIR/MERGE_MSG` | merge state | keep |
| 2095 (`read_head_oid`) | `$GIT_DIR/{MERGE_HEAD,CHERRY_PICK_HEAD,REVERT_HEAD,REBASE_HEAD}` | pseudoref | keep |
| 2410 | `rebase-merge/head-name` / `rebase-apply/head-name` | sequencer state | keep |
| 2422 (`read_orig_head`) | `$GIT_DIR/ORIG_HEAD` | pseudoref | keep |

**Result: 10 direct-read sites — all pseudoref or sequencer state, 0 real-ref
reads. Nothing to migrate.** libgit2 does not expose the sequencer/pseudoref
state these functions need, which is why they read the files directly; that
remains correct under reftable.

Why these are read directly rather than via git2: libgit2 has no API for
in-progress sequencer state (rebase step counts, `MERGE_MSG`, the
cherry-pick/revert heads), so `conflicts.rs` reads the state files git itself
maintains. This is orthogonal to the ref backend.

### Related (not a read, noted for the reftable follow-up)

`src/ui/watcher.rs` triggers a graph reload on filesystem events whose path
component matches `refs` / `packed-refs` (`GIT_STATE_NAMES`). On a reftable
repo, ref updates land in `.git/reftable/` and would **not** match these names,
so a branch move might not fire a `WatchEvent::Git` reload. This is a
notify-watch classification gap, not a direct read, and cannot be verified
without git ≥ 2.51. **Deferred** to the reftable-enablement work (add
`reftable` to `GIT_STATE_NAMES`, or watch `$GIT_DIR` more broadly). Recorded
here so it is not lost.

## Decision — Part 2: `add -p` split UX — proven negative, no change

git 2.52 fixed an interactive `add -p` bug: after selecting a hunk, splitting
it left every split piece marked *selected* instead of resetting them to
*undecided*.

**Kagi does not have this bug, because kagi has no interactive per-hunk
staging.** Verified by enumerating the staging surface:

- `crates/kagi-git/src/staging.rs` stages **whole files only** —
  `stage_file` / `unstage_file` / `stage_files` / `unstage_files`. There is no
  hunk selection, no line selection, and no split operation.
- The only per-hunk selection state in the codebase is the **Conflict Editor**
  (`resolution.rs` `apply_hunk_choice`, `conflict_view.rs` `selected_hunk`).
  Those hunks are fixed by the diff3 conflict markers — the user accepts a side
  or line, they cannot *split* a hunk, so the "split resets selection" bug is
  structurally impossible there too.

No `add -p`, no `git add --patch`, no `stage_hunk` path exists (grep for
`stage_hunk` / `add.*-p` / partial-stage: no matches). There is no selection
state to reset on split, so there is nothing to fix and no test to add — a
proven negative. Should kagi ever gain interactive hunk staging, that new
feature must reset split pieces to undecided from the start (issue #357 §4a).

## Deferred (untestable on git 2.50.1)

- **`git url-parse` (2.55)** — delegating SSH/HTTPS/scp-like remote-URL parsing
  in `github.rs` owner/repo inference to git itself. Needs git ≥ 2.55; a 2.50
  fallback (current self-parse) must stay, so this is a two-path maintenance
  decision (issue §5). Deferred.
- **`hook.<event>.enabled=false` (2.52)** — using per-hook config to disable a
  hook honestly in the plan instead of `--no-verify`. Open question in the issue
  is whether kagi may rewrite repo config vs. pass `-c` transiently. Needs
  git ≥ 2.52. Deferred.
- **`git repo info -z` reftable detection (2.52)** — reading
  `references.format` to detect a reftable repo. Needs git ≥ 2.52. Deferred; the
  watcher gap above is the concrete first task once a reftable repo can be
  created (`git init --ref-format=reftable`, git ≥ 2.51).
- **libgit2 → `libgit.a` migration** — explicitly out of scope (revisits
  ADR-0002), recorded in issue §7.

## Consequences

- No code change to the git or UI layers this PR — the audit's outcome is that
  kagi's direct reads are already reftable-safe and the `add -p` bug does not
  apply. The value is the documented inventory (issue §6 acceptance) and a
  concrete deferred-work list with the exact APIs and git versions required.
- The one actionable reftable gap found (watcher ref-name matching) is captured
  for the follow-up rather than fixed blind, since it cannot be verified on the
  local git.
