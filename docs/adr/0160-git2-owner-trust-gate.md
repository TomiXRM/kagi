# ADR-0160: An owner-trust gate for the git2 (libgit2) path

- Status: Accepted
- Date: 2026-09-03
- Closes: #310
- Continues: ADR-0146 (its Consequences deferred exactly this gate)

## Context

ADR-0146 hardened the **CLI** path (`run_git`) against a hostile repository.
git 2.35.2+ performs its own owner check on every CLI invocation, so `run_git`
inherits `safe.directory` enforcement for free — a repo owned by another user
makes `git` refuse before running any of its config's executable keys.

The **git2 (libgit2) path has no such enforcement.** libgit2 does not honour
`safe.directory` at all: `Repository::open` on a foreign-owned working tree
succeeds silently, and everything reachable from `Backend` — including writes —
then runs against a tree an attacker may own. ADR-0146 explicitly deferred this
(its "Deferred: the owner / `safe.directory` gate" note), because:

1. the realistic attack is *clone/download a hostile repo you then own*, which
   an owner check does not stop (the CLI hardening in ADR-0146 covers that);
2. a non-UI **hard refusal** would brick a legitimate shared or root-owned repo
   with no opt-in path, so the gate only makes sense paired with a trust UI.

This ADR adds that gate and its trust UI.

## Decision

The gate lives at **`Backend::open`** (and `discover`), the one place a git2
`Repository` is opened for a tab, in `crates/kagi-git/src/trust.rs`.

### Trust evaluation

A repo workdir is **`Untrusted`** iff *all three* hold:

1. the workdir is owned by a **different uid** than this process, AND
2. it is **not** covered by git's own `safe.directory` config, AND
3. it is **not** in our own `trusted_repos` store.

Otherwise it is `Trusted`. The decision is a pure function,
`evaluate_trust(canonical_path, foreign_uid)`, with the uid comparison lifted
out as a parameter so the branch logic is unit-testable without a `chown`
(which test sandboxes forbid).

- **Ownership** is `std::os::unix::fs::MetadataExt::uid()` compared to this
  process's effective uid. std exposes no `geteuid`, so the euid is learned
  once by stat-ing a freshly created temp file (the kernel stamps it with our
  euid) rather than pulling in `libc`. On non-unix the check is a no-op (allow).
- **`safe.directory`** is read from `git2::Config::open_default()` — the
  user/global + system scopes, **not** the repo-local config (which git ignores
  for `safe.directory` by design, and which an attacker controls). Values `*`
  and an exact absolute path are honoured; `%(prefix)`-relative and `/*`-subtree
  forms are not expanded (a user relying on those still gets one trust prompt).
- **Trust store** `trusted_repos.json` lives beside the oplog
  (`$KAGI_LOG_DIR` first, else `$HOME/.kagi/`), keyed by canonical repo path.
  Named `trusted_repos` to stay distinct from any worktree-config trust.

### Semantics: read-allowed, write-blocked

Opening an untrusted repo does **not** fail — inspection/read is allowed so the
user can look before deciding. Only **`Backend::run`** — the single enforced
entry point for every mutating operation (ADR-0104) — refuses, at its very top,
with a typed `GitError::Untrusted`, recorded to the oplog as a `Failed` attempt
(ADR-0149) so no write path has an unlogged hole.

### Trust UI

`Backend::trust()` exposes the state. On tab open the UI raises a
trust-confirmation modal (`TrustRepoModal`, `src/ui/trust_prompt.rs`,
`open_/confirm_/cancel_` per ADR-0076). Confirming calls `trust::trust_repo`
(persist) and **re-opens the session** — the write worker caches trust at spawn
(ADR-0073), so a re-open is the clean way to invalidate it. Strings are EN+JA
(`TrustRepoTitle/Body/Confirm`).

**Headless never auto-trusts:** it has no prompt, never calls `trust_repo`, so
an untrusted repo simply stays read-only there.

## Consequences

- A foreign-owned repo opened via the git2 path can be inspected but cannot be
  mutated until the user confirms trust; the refusal is enforced at
  `Backend::run` regardless of whether the UI prompt fired, and is surfaced via
  the oplog and the error footer.
- A legitimate shared / root-owned repo is usable: the user grants trust once
  (persisted in `trusted_repos`), or has already set git's `safe.directory`,
  which the gate honours.
- The uid comparison is injected as a parameter (`evaluate_trust`) so the trust
  logic is unit-tested directly; the write-block is integration-tested through a
  documented `Backend::set_trust_for_test` seam, since sandboxes cannot `chown`.
- Supersedes ADR-0146's deferral of the owner gate for the git2 path.

### Follow-ups / known ceilings (`ponytail:`)

- `safe.directory` matching handles `*` and exact paths only; extend to git's
  full `is_path_safe` (prefix/subtree forms) if such configs prove common.
- The trust prompt fires on tab open. Extending it to also pop from a
  write-refusal path (so a repo dismissed once still offers a grant on the next
  write attempt) is a small UI follow-up; the git-layer block protects writes
  either way. Both need GUI eyeballing (a subagent cannot exercise the GUI).
