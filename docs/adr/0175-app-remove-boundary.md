# ADR-0175: Worktree remove application boundary (slice 1a)

- Status: Implemented for review; PM verification (including M) pending
- Decision: approved DESIGN r2 / #484 / #521

KagiApp owns a minimal Sessions field, not a second global host. A window-free
plan/Ready/approve/prepare/run/apply API owns approval and leases. Existing
remove primitives remain in kagi-git; a dedicated execution boundary opens a
fresh Backend, validates the frozen admin fingerprint, captures progress outside
unwind, verifies and records before delivering completion. The UI never records
remove a second time. append_oplog delegates to a receipt-returning sibling.

Unknown is an additive persistent outcome with after/recovery and evidence.
Old outcome encodings remain unchanged. A stopped unknown requires read+ack;
unconfirmed termination retains its lease. Normal window close/Quit is held
while a lease exists. Tab close keeps KagiApp alive at Welcome.

This partially supersedes [ADR-0149](0149-oplog-in-backend-run-and-schema.md)
for remove's writer and observed after, [ADR-0104](0104-enforced-operation-pipeline.md)
for the dedicated non-run boundary, and [ADR-0107](0107-repo-session.md) only
for remove's fresh mutation handle (read RepoSession is unchanged).

No worker, TabId, session incarnation, global registry or writer admission
rollout is included. Slice 1b must connect editor save/staging/snapshot/fetch
before family rollout. The original bare ODB backup limitation is addressed for new file backups by
[ADR-0179](0179-ref-backed-discard-remove-backups.md) (#523): mandatory refs
survive GC and are retained with their oplog entries.

## Concrete implementation and compatibility

- `src/app/{session,worktree}.rs`: opaque approval tokens bind the request,
  policy/actor and monotonic revision. `prepare_remove` consumes approval and
  reserves the canonical common-directory RepoId. `LegacyBusy` is documented
  for removal at the final family migration. No new crate or runtime.
- `backend/remove.rs`: fresh owner-trust checks for management and target,
  admin inode/birthtime + gitdir bytes + config digest + linked HEAD/ref,
  existing preflight, config trust grant, executor, observed result, receipt.
  Missing identity attributes fail closed (no path-only fallback).
- `ops/worktree_remove.rs`: existing remove plan/executor/backup helpers moved
  together out of lifecycle.rs; other lifecycle bodies are unchanged. Progress
  is passed by mutable reference from outside catch_unwind, updated before
  side effects and immediately after each backup. Verification is explicit.
- `Unknown { after, evidence }` is additive. Evidence is a human-readable
  string carrying stage, verification, termination and step observations.
  Partial/Unknown after contains full blob and branch OIDs. Existing variants
  retain byte-for-byte serialization rules and old logs remain readable.
- `append_oplog_receipt` returns `(path, assigned_entry)`; `append_oplog`
  delegates. On append failure the boundary returns the attempted entry plus
  error, independently of the mutation outcome. No tail-based receipt lookup.
- `KagiApp::dispatch_job` settles before attachment checks. Deleted targets
  invalidate cache without reload; inactive owners reload on next tab switch.
  Error notices and read/ack tokens are bound together, never attached to a
  different repo's approval modal. Dropped unrun jobs record failure and queue
  completion locally in Sessions; no global mailbox is introduced.
- Normal native window close and app Quit menu/shortcut share the lease guard.
  保証するのはアプリ内 close/Quit 入口の保留のみ（PM 承認）。
  Forced process termination is outside the contract. The locked GPUI exposes
  no OS termination veto: Dock/OS-level termination is not claimed to be held;
  a platform change for that path requires a separate decision.
- `with_fault_for_test` selects a finite fault on a private job field (normal
  None); no env/GUI/tool input maps to faults. uv gates prevent callers outside
  tests and prohibit direct I/O/UI dependencies in src/app. Samples self-test
  both gates. Native E2E's feature-only bounds probe observes the actual confirm
  button so the test can click it rather than invoke its handler directly.

G covers ordinary/refused/revision/drift/ABA/open-failure/duplicate/close and
delivery, abandoned jobs, partial backup, internal executor panic with JSONL
recovery, failed read/replayed ack, unconfirmed termination, append failure,
receipt races and Unknown round-trip. E drives raw Enter and an actual button
click through the real remove modal. M belongs to PM, not this agent.

## Slice 1b: competing writer admission (#484)

Implemented for review; the 1a+1b rollout gate remains a PM decision.

- Extend the same Sessions lease table with an owned `WriteGuard`, not a new
  registry. `write_lease(path, LegacyBusy)` resolves a canonical common
  directory through Backend, checks Busy/NeedsReconcile and reserves before
  returning. All repositories remain mutually exclusive. `reserve_write` mirrors
  busy in the same host turn; only that mirror is cleared once no lease remains.
  Identity lookup is read-only and must not require Git trust: plain editor
  saves retain ADR-0120 behavior. Git writer facades keep their own trust gates;
  both fetch facades now explicitly require trust before invoking the CLI (#502 C2).
- The table is shared with guards using Arc/Mutex so executor completion does
  not depend on the editor/window still existing. Explicit `complete` releases
  only its own identity/id. Ordinary Drop and panic retain the lease, never
  pretend an unknown writer has stopped. Poisoned locks fail closed.
- Editor save/overwrite freezes its existing payload and emits SaveRequested.
  The host reserves and seeds `save_reserved` with an owned completion callback.
  Only then does the existing pane executor dispatch. The editor gains no app,
  Git or new crate dependency. Its disk comparison and write logic are unchanged.
- All six panel/editor staging entry points reserve around the existing sync
  calls, including early error paths. Manual snapshot capture/pruning likewise
  retains its current synchronous scheduling and log contract.
- Manual/auto/branch fetch reserve before spawn. The guard lives in the actual
  background future, not a generation-guarded presentation callback. Existing
  `fetch_in_flight` remains; both forward and reverse contention now use leases.
- CLI timeout/reap uncertainty has an additive `GitError::TerminationUnknown`
  tag with the exact previous Display text; fetch cannot mistake it for a known
  failed process and release admission. No executor relocation or process-tree
  change is made. Such uncertainty intentionally keeps writes/window close
  blocked; termination recovery remains follow-up work, not an automatic Drop.
- G exercises the public reservation API with real fixture save/stage/snapshot/
  fetch writers, both orderings with remove, saved bytes, canonical identity,
  read+ack, cross-thread completion and dropped/panicking/unknown guards.
  E holds a lease through the real host's Sessions API, then drives editor
  keystrokes and SaveRequested: Busy toast/footer, dirty buffer and unchanged
  bytes, followed by release and successful re-save. This proves adapter wiring
  without child-process scheduling; real remove contention is covered by G,
  and slow pre_remove versus editor save is reserved for PM's manual M check.
  The agent builds E only; PM executes it and the workspace suite.

This closes only the named GUI bypasses. Other writer families and independent
repo concurrency are not enabled; LegacyBusy is removed at the last migration.

### Filesystems without birth time (#587 release review)

The admin fingerprint stores creation time as `Option<SystemTime>`.
`Metadata::created()` returning `Unsupported` contributes `None`; other errors
still refuse planning. Inode, gitdir bytes, optional config SHA, HEAD OID and
HEAD ref remain mandatory comparisons. Two observations lacking birth time can
match; a changed/missing-versus-present birth time or any changed remaining field
still requires re-planning. This does not relax the existing Unix inode boundary.
