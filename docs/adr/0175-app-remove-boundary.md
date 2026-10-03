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
- #915 / #938: linked 自身からの Remove でも削除境界を実行元の workdir に置かない。main workdir **と** common dir の両方を常に守る。non-bare でも `--separate-git-dir` の common dir が削除対象内にある場合は計画・実行前・削除直前に拒否する。削除前 copy / symlink ステップの source は main workdir が無い場合に実行元 linked worktree を使い、削除境界と混同しない。
- #938 review: `Repository::open(common).workdir()` は `--separate-git-dir` で `core.worktree` が無くても推測した path を返すことがあり、main が ignored フォルダーへ移動していると self-remove が main を巻き込む。main の証明は (1) common dir が main の親ディレクトリ直下の `.git` **ディレクトリ**、または (2) `core.worktree` があり、その workdir の `.git`(gitfile / ディレクトリ)を開いた git dir が common dir そのもの、とする。bare は main workdir が無い。いずれでも証明できない non-bare は計画・preflight・削除直前に短い EN/JA blocker で拒否する。通常の `.git` 配置での own-tab Remove は維持し、common dir から推測した workdir は保護根拠にも削除前ステップの source にも使わない。
- #938: recorded Remove の前後 snapshot は管理元が残る場合はその
  worktree の HEAD、管理元自身を削除する場合だけ surviving common dir
  の HEAD を観測する。同じ source を前後で使い、削除前 step による
  管理元の checkout は HEAD 移動として記録して restore を拒否する。
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

## Busy 表示と lease の終端化 (#607)

`reserve_write` は操作名を受け取り、`dispatch_job` は既存の job 名を使う。
`busy_op` に汎用の内部識別子を入れず、lease に由来する mirror 名を別に保持する。
lease がなくなった時だけ一致する mirror を解除し、lease を持たない legacy plan は解除しない。
表示名は ui-core の EN/JA 対応表に集約し、未知名は汎用の「処理中…」にする。
uv `check-busy-labels` は busy 名の literal と有限な operation/job 名を照合する。
この表示変更は lease admission・実行・記録・既存 klog 契約を変更しない。

## Read-only worktree inspection (#633)

The WORKTREES navigator adds a capacity badge and a read-only detail card shown
while hovering the corresponding local worktree row. Neither creates an approval
token nor changes removal eligibility. Planning, confirmation, preflight,
execution, verification, oplog, backups and lock semantics stay on the boundary
above. The hover card does not reserve space below the five navigator panes.
It identifies the hovered worktree by name, branch and visually truncated path
(the full sanitized path remains its AX name) and stays interactive when the
pointer moves from the row into the card to refresh. An icon-only Refresh
control keeps an accessible name. Its height stays stable as a measurement
starts or finishes, preventing GPUI from flipping the tooltip to the opposite
side of the pointer during Refresh.
The native tooltip paints after KagiApp's root overlays; while the worktree
context menu is open, the card yields completely so the menu remains visible
and clickable. The navigator lists only linked worktrees: the main worktree is
not removable, keeps its graph navigation and shared-port role, and does not
need a capacity scan just to populate a hidden row.

- `kagi-domain::remove` owns typed `WorktreeEvidence`, unknown reasons and the
  pure advisory predicate: clean **and** unlocked **and** (merged **or** pushed).
  The main worktree is excluded. Missing observations never become positive
  evidence; missing upstream, unborn HEAD and observation failures remain
  distinct EN/JA reasons.
- `kagi-git::worktree_inspection` reopens the repository and re-observes registry
  identity, lock, status and HEAD rather than trusting cached WIP metadata.
  Merged means ancestry in the existing default-branch policy; pushed means
  ancestry in the locally available remote-tracking upstream. Both the refspec
  result and its resolved symbolic target must remain under `refs/remotes/`;
  mirror refspecs or aliases into local refs are not publication evidence.
  There is no network request, automatic fetch or PR query.
- Capacity is allocated filesystem space, including ignored contents, not file
  length or promised reclaimable space. The in-process walk splits `target/`
  subtrees from other contents, does not follow symlinks and counts hardlinks
  once per worktree. Unix uses allocated blocks. Windows reads
  `FILE_COMPRESSION_INFO.CompressedFileSize` for files on the same no-follow
  handle used for identity, and directory allocation separately. Unsupported
  physical-size queries never fall back to a file's `AllocationSize`.
  Unreadable, disappearing or unsupported entries fail the capacity result
  instead of returning a misleading partial total. APFS clones
  and externally linked files mean removing a tree may free less than its sum.
- The existing session-owned `TabUiState` stores cached reports and measurement
  time. A background scan checks cancellation per entry. Supersession, hovering
  another row, tab departure and close retire the request; both read
  freshness and request revision guard delivery. A stale read or active writer
  suppresses a cached positive verdict, while the last capacity remains visible.
  Initial observation and explicit remeasurement share this path and target
  only linked worktrees. Coverage is per displayed worktree: tab return
  schedules missing and newly added linked targets while preserving completed
  cached observations. The renderer performs no I/O and does not repeatedly
  rescan cached worktrees.
- #934: the hover card is a compact **read-only observation**, not a removal
  guide. It shows name, branch, concise Git-state chips, a local port shortcut
  when available, path and occupied bytes. It keeps the last measured bytes
  during a pending/stale scan but does not advertise a stale positive verdict.
  Tree/branch/lock/terminal/refresh are registered vector icons, not emoji
  or textual instructions. Refresh is icon-only with an AX name; path
  truncation is visual only (full sanitized path in AX). Hover and ordinary
  reload never initiate removal or scan ignored files for a deletion warning.
- For **Remove worktree**, `ops/worktree_remove.rs::plan_remove_worktree`
  observes ignored entries in the target linked worktree using read-only
  `git2` status. If any exist, the plan adds a typed EN/JA warning counting
  ignored files and top-level ignored folders. A wholly ignored directory is
  one folder, not a recursive file count. These items are not in the
  uncommitted-content backup; the warning belongs to the confirmation plan,
  not to the hover observation. The confirmed warning carries the approved
  `(files, folders)` (or `(0, 0)` when absent). Preflight re-observes both counts
  before trust or `pre_remove` steps, and the executor re-observes again after
  those steps and backup, immediately before directory deletion. An increase
  in either category refuses removal and requests a fresh confirmation; an
  unreadable status also refuses deletion. The post-step refusal is a partial
  receipt because those steps may have changed files, but the worktree and
  branch are kept. Counts do not attest to new files *inside* an already
  ignored folder: that folder is deliberately treated as one entry. No-force
  and lock semantics stay unchanged. SSH worktree paths never enter this
  local observer.
- A linked worktree with a populated gitlink path is blocked from Remove.
  This includes initialized submodules and **uninitialized** submodules whose
  directory contains even one local entry: Git status excludes those bytes,
  and the non-force checked deletion would otherwise erase them. Planning
  inspects only tracked gitlinks and their immediate directory entries,
  without recursively scanning or following a symlink at the gitlink path.
  An empty directory or absent gitlink path is allowed; Remove's linked-worktree
  dirt check exempts only an absent gitlink's worktree deletion status when
  there are no other changes. Staged gitlink updates, other worktree dirt and
  all shared status reads remain blocking/unchanged. A non-directory occupant
  blocks rather than being followed.
  Preflight repeats the check before trust or steps; execution repeats it after
  `pre_remove` and immediately before directory deletion. Unreadable
  submodule entries fail closed. The typed EN/JA blocker is short, with
  no procedure text. A post-step Partial receipt retains complete stage
  evidence in the oplog, while its toast shows only the localized blocker.
  Refusal does not delete the worktree, submodule files or branch; an approved
  `pre_remove` step may already have changed files when the refusal occurs.

- #938: Remove の削除対象内に**別の登録済み worktree**（main を含む）
  がある場合、実行元の worktree に関係なく typed EN/JA blocker で拒否する。
  対象自身の登録だけは除くため、対象自身のタブからの Remove は許可する。
  Git の登録パスを列挙・正規化して確認し、読み取りに失敗した場合は削除しない。
  同じ判定を plan、承認後の preflight、`pre_remove` とバックアップを
  終えた recursive delete の直前に行う。ignored フォルダーが既にある場合、
  内側の worktree を後から作っても ignored 件数は増えず、既存の dirty /
  ignored / main-root チェックだけでは内側の未バックアップの内容を守れない。
  preflight の拒否は Refused、`pre_remove` 後の拒否は Partial receipt
  として oplog に詳細を保ち、内側・外側の worktree と branch を削除しない。

Pure verdict tests and filesystem/Git fixtures cover evidence precedence,
hardlinks, symlinks, ignored allocation and cancellation. The focused native
`worktree_inspection` scenario uses real pushed/dirty/locked/detached worktrees,
hovers visible virtual rows, verifies the card disappears when the pointer
leaves, checks EN/JA chip states and manual remeasurement, and holds only report
delivery to prove superseded, departed and closed owners do not accept late
results. Tier B reviews the card and removal warning; Windows
cross-compilation is not a claim of Windows runtime validation.

## Typed remove refusal delivery (#353)

Initial preview blockers must not become the generic `plan has blockers` error.
The existing `GitError::Blocked` → `RemoveReport.blocker` sidecar follows the
conflict-family precedent (#711); planning errors preserve `PlanState::Error.blocker`.
The durable Refused receipt retains **every** initial blocker in English, while
the first typed blocker reaches the existing EN/JA `plan_note_text` presenter.
That typed refusal uses the same AppNotice and bounded-toast presentation as
conflict refusals. Untyped failures, partial/unknown settlement, admission and
the preflight/execution order remain unchanged. No other family is migrated.

`app_remove_test::refused_dirty_locked_main_missing` covers multiple simultaneous
reasons and main/missing targets. Native `remove_public_boundary` additionally
confirms a locked plan through Enter in EN/JA, verifies the notice and bounded
toast, one reason-bearing receipt, released lease, and unchanged repositories
and worktree lock. Its existing successful Enter/button removals remain covered.
