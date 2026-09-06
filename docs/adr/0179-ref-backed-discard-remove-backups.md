# ADR-0179: Ref-backed discard and worktree-remove backups

- Status: Implemented for review
- Issue: #523
- Related: [DESIGN §5.3 / §8.1](../rearch/app-layer/DESIGN.md),
  [ADR-0175](0175-app-remove-boundary.md),
  [ADR-0046](0046-discard-changes.md),
  [ADR-0154](0154-working-tree-snapshots.md)

## Mandatory recovery roots

Discard and remove previously wrote naked ODB blobs. An oplog string containing
an OID is not a Git reachability root; even automatic Git GC can prune it.
Both existing executors now call the same small `ops::backup` helper, which
writes the blob and creates an exclusive direct ref before returning a backup:
`refs/kagi/backups/<attempt-id>/<file-ordinal>`. A direct blob ref is a GC root;
a new commit/tree or mutation of the index is not required for byte recovery.
The attempt id combines nanosecond time, process id and a process-local counter.
It is independent of the post-execution oplog sequence id and the application
OperationId. Ref creation never overwrites: a collision stops before deletion.
Ordinals keep filenames out of Git ref syntax. Linked worktree removal creates
these refs in the management/common repository, which survives path deletion.

Backup remains mandatory independently of auto_snapshot. Snapshot cap pruning
never visits this namespace. No GUI state, app-layer I/O, new service, or
optional trust/preflight/verify flag is introduced. Existing removed-branch full
OIDs and their reflog recovery remain a separate mechanism; this change pins
file-content backups, not arbitrary branch histories or hook external effects.

`DiscardBackup` retains its full blob OID and adds the exact ref name. Structured
`OpLogEntry.backup_refs` is additive (old records read as an empty list). Existing
path/OID summary prefixes remain, with a recovery-ref mapping appended. Discard
records actual backup evidence, not its predicted summary. Remove uses the
unwind-external progress from ADR-0175 for Success, Partial and Unknown alike.
A failed append returns an attempted receipt containing the refs; no Drop or
error cleanup removes recovery roots. Failure part-way through backup before
any deletion may leave extra pinned objects; preserving bytes takes priority.

## Recovery

Use the receipt's exact ref in `git cat-file blob <backup-ref>` or
`Backend::read_backup(reference)`. The latter returns bytes and never writes a
working-tree file; a future overwrite UI still needs a separate approved write
plan. The reader accepts only exact refs in the backup namespace, not revision
expressions or bare OIDs. Symlink backups still contain link-target bytes, not
a recreated filesystem symlink. This preserves the existing discard contract.
Legacy naked-OID receipts remain readable, but are not retroactively pinned or
claimed GC-safe; objects already pruned cannot be reconstructed by this change.

## Retention and cleanup

**Default: the same lifetime as the corresponding oplog entry.** The current
oplog has no automatic age/count expiration, so backup roots are likewise kept
indefinitely. There is no 50-entry snapshot cap or day-based backup expiration.
No GUI/CLI cleanup command or background collector is introduced in this PR.

`Backend::plan_forget_oplog_entry` provides a frozen preview of one entry and
its exclusively owned recovery refs. Explicit confirmation authorizes
`execute_forget_oplog_entry`: current owner trust, repository/common-dir and log
identity, unchanged log bytes, and locked direct-ref OID checks are mandatory.
Another retained entry referencing the same ref keeps that root alive. Invalid
log lines, wrong namespaces, missing/ambiguous entries and drift fail closed.
Retained lines are preserved verbatim, including unknown fields. The original
repository must still be resolvable for this operation; deleted/moved unrelated
worktree receipts fail closed rather than guessing a repository from a name.

Execution atomically replaces the log first, then removes only the planned ref
names under Git ref locks and verifies absence. Replacement is synced before
ref removal (including the parent directory on Unix). An error before log
replacement is Failed; after replacement it is Partial, with cleanup candidates
in the actual invocation's recording. Cleanup errors can leave extra roots;
there is no namespace sweep, retry of the original Git mutation, or deletion of
another receipt's roots. The cleanup itself has one Backend finalization attempt.

Append and retirement share a stable sidecar file lock; a busy lock refuses the
attempt instead of blocking the UI thread or overwriting another writer's log.
The sidecar also reserves the next sequence id before writes so retirement of
the newest/only entry cannot reuse its id. Failed writes may leave sequence gaps.
This is local oplog coordination, not a cross-resource crash-recovery journal:
external log editing, old executables that ignore the lock, manual ref deletion,
and arbitrary process/power failure are outside the transaction contract.
Orphans from interrupted backup/failed append/failed cleanup stay pinned until
explicitly reviewed by an administrator. Missing log files are never interpreted
as authority to delete all refs. Copying/archiving logs externally does not add
another managed retention owner; export bytes before explicitly retiring an entry.

## Evidence

`crates/kagi-git/tests/ref_backups_test.rs` disables optional snapshots and runs
real fixture `git gc --prune=now`, with a sibling naked blob that must disappear.
Persisted discard and remove Success/Partial/Unknown receipts still recover the
original bytes through refs. Other G cases cover append failure, ref-creation
failure with unchanged worktree, deletion of only an expired entry's roots,
shared-root lifetime, post-plan log/ref drift, trust refusal, malformed records,
namespace confinement and monotonic ids after retiring the last entry.
GUI runner execution is prohibited; this PR claims Git fixture evidence only.
