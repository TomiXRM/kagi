# ADR-0154: Working-tree snapshots under `refs/kagi/snapshots/`

- Status: **Accepted**
- Date: 2026-09-03
- Related: ADR-0046 / ADR-0083 (discard ODB-blob backup), ADR-0149 (#333 oplog id),
  ADR-0104 (enforced operation pipeline), #335, #282, #334 (oplog panel)

## Context

Kagi has a discard-time ODB-blob backup (ADR-0046 / 0083): before a discard it
writes each target's bytes to the ODB via `repo.blob()` and records the SHA in
the oplog. That backup is **discard-only** and — as #282 confirmed — a *loose,
unreferenced* object, so `git gc --prune=now` deletes it. There is no way to
save an arbitrary working-tree state and be sure it survives.

The AI-native motivation (#335): Claude Code / Cursor checkpoints are explicitly
*not git*, so they cannot capture bash- or subagent-driven edits. If Kagi records
a savepoint **inside git**, then whatever an agent does is recoverable in git's
own terms. But "a git client keeps snapshots outside git" is self-contradictory,
so the savepoint must live in the normal ref namespace — just outside
`refs/heads` / `refs/remotes`.

## Decision

A **snapshot** is a real commit of the working tree + index, written to the ODB
and pointed at by a ref under `refs/kagi/snapshots/<id>`.

- **Survives gc.** A ref is a reachability root, so the commit is never pruned —
  this is the property the discard blob backup lacks (asserted in
  `tests/oplog_snapshot_test.rs::snapshot_survives_gc_and_restores`, which also
  shows a sibling loose blob *is* pruned).
- **Invisible to branches.** `refs/kagi/` is outside `refs/heads` / `refs/remotes`,
  and branch listing (`repo.branches(Local)`, `crate::snapshot::collect_branches`)
  and the commit-graph walk (`log.rs` globs `refs/heads|remotes|tags`) only look
  there — so a snapshot never appears as a branch.
- **Never pushed / fetched.** Push always names the current branch explicitly
  (`ops::push` builds `git push -- <remote> <branch>`, no wildcard refspec) and
  fetch refspecs come from the remote config, so `refs/kagi/` is never a target.

### Tree capture

`create_snapshot` builds the tree with `git add -A` semantics via a temporary,
never-persisted index mutation (`Index::add_all` then `write_tree`, then
`index.read(true)` to discard): tracked modifications **and** untracked-but-not
ignored files are included, `.gitignore` is respected (so `node_modules` is not
swept in — #335 §5). The user's on-disk index is never written.

### Id

`<id>` aligns with the oplog sequence id (#333) when the oplog is readable, and
falls back to a Unix timestamp otherwise. Uniqueness is guaranteed by bumping
past any existing ref (two manual snapshots before any op would otherwise peek
the same oplog id).

### Retention (PM-locked, #335 §5)

Generation cap, **default 50** (`DEFAULT_SNAPSHOT_CAP`), plus explicit delete.
**No day-based expiry** in v1. `prune_snapshots(cap)` evicts the oldest beyond
the cap (deleting a ref only — non-destructive to files). The auto-snapshot path
prunes after each create; the manual UI path prunes after a manual snapshot.

### Two entry points (#335 §4)

1. **Explicit** (the core): a "Create Snapshot" command (Repository menu +
   command palette) → `Backend::create_snapshot`. Non-destructive, so no
   plan/confirm — just the action and a toast.
2. **Automatic**: before a **destructive** op mutates the repo, `Backend::run`
   takes a savepoint, gated by the `auto_snapshot` setting (default on;
   `Backend::set_auto_snapshot`, wired from `Settings::auto_snapshot()` in
   `blocking_ops::open_backend`). Realised in `run` right before dispatch (once
   per executed op) rather than literally inside each `plan_*` — planning is
   re-run many times while the confirm modal is open, and snapshotting on every
   replan would be wasteful. The guarantee ("a savepoint exists before the repo
   is destructively mutated") is identical.

### Restore

Restore rewrites the working tree, so it is a full write op:
`plan_restore_snapshot` / `preflight_restore_snapshot` / `execute_restore_snapshot`
/ `verify_restore_snapshot`, dispatched through `Backend::run` as
`Operation::RestoreSnapshot` so it is recorded in the oplog (asserted in
`restore_goes_through_plan_and_oplog`). Execute order is **savepoint → checkout →
verify**: a fresh savepoint of the current tree is taken first (restore is itself
reversible), then `checkout_tree` with force overwrites/creates the recorded
files. **No `reset --hard` and no `git clean`** (invariant #3): restore is
additive — it never deletes files that were created after the snapshot — so
`verify_restore_snapshot` is a *subset* check (every recorded blob matches on
disk), not exact tree equality. The restore **UI** (a snapshot list + confirm)
belongs to the oplog panel (#334, which the issue names as the restore home);
this ADR delivers the tested backend + plan path it consumes.

## Coexistence with the discard backup (PM-locked)

The discard ODB-blob backup (ADR-0046/0083) **stays** — replacing it now would
interfere with the #281/#282 fixes. A future migration folds discard's blob
backup into a tree-level snapshot (its `gc` weakness disappears there); that is
explicitly out of scope here.

## Consequences

- New pure types: `SnapshotNote` / `SnapshotTitle` / `SnapshotRecovery` (a
  `plan_note` category) and `Operation::RestoreSnapshot`.
- New git-layer module `crates/kagi-git/src/ops/snapshot.rs`
  (`create_/list_/prune_/delete_snapshot`, the restore triple + verify) and
  `Backend` facade methods.
- New setting `auto_snapshot` (default on) and Msg strings `SnapshotCreated` /
  `SnapshotFailed` (EN + JA).
- `refs/kagi/` snapshots accumulate ODB objects; the generation cap bounds this.
  Auto-snapshotting large working trees before every destructive op has a cost;
  the toggle exists for users who want it off.
