# ADR-0177: Backend execution policy and public mutation contracts

- Status: Implemented for review
- Issues: #494, #502
- Related: [application design §5](../rearch/app-layer/DESIGN.md),
  [ADR-0175](0175-app-remove-boundary.md), [ADR-0176](0176-app-stash-local-boundary.md),
  [ADR-0154](0154-working-tree-snapshots.md), [ADR-0160](0160-git2-owner-trust-gate.md)

## Policy ownership

`backend::ExecutionPolicy` holds actor and optional auto-snapshot preference.
It contains no trust/preflight/verify switches. `open_with_policy` and
`discover_with_policy` apply these values together; plain open/discover retain
Human + snapshots-on compatibility defaults for read and fixture consumers.
GUI mutation/plan handles use `ui::blocking_ops::open_backend`, whose single
resolver reads Settings. Reset, rebase, force-with-lease and synchronous
amend/merge/checkout now use that same factory as background/button operations.
The stash application's already approval-bound policy reuses this value type.
No settings read, GPUI, git2 primitive or I/O is added to `src/app`.

CLI and MCP use explicit Cli/Mcp policies with snapshots **on**, independently
of GUI settings. No tool argument can disable a safety gate. Worker submissions
accept policy per request and fresh-open at execution, so trust is re-evaluated
at the same boundary as a direct write. The worker remains unconnected to the
production GUI; this does not perform #314's migration or change admission.
Legacy worker submit keeps its documented Human/on default; new adapters use
`submit_with_policy`. Mutation gates also re-evaluate owner trust on direct
long-lived handles; a cached untrusted handle still requires re-opening after a
grant. Target paths remain explicit, never resolved from the current UI tab.

Legacy GUI families resolve Settings at execution through the common factory;
stash binds policy at plan/approval as in ADR-0176. Migrating all legacy modal
approvals to the application token is still family work under #484. This change
does not claim those old modal types already bind a policy revision.

## Public mutation inventory

T = owner trust at execution; F = backend preflight; V = verification owned by
the family, not an optional UI callback; S = recovery; R = existing record owner.
“Explicit request” permits the established index/metadata UI without a modal.
The application owns approval/admission; it cannot grant trust with a boolean.

| Public boundary | Approval / T / F / V | S / R and reason for exceptions |
| --- | --- | --- |
| `run` / `run_recorded`: commit, merge-commit, amend, undo-commit, cherry-pick, revert, reset, rebase | Confirmed plan; T; HEAD and required worktree digest, fresh family blockers and safety shape; family execution checks | Existing destructive-plan auto snapshot toggle; one Backend finalizer. Rebase is Guarded and retains its existing no-auto-snapshot rule. |
| Same: checkout, detached/tracking/latest checkout, create-branch-with-checkout | Confirmed target; T; HEAD/dirty/ref requirements; safe checkout and ref result | Existing auto classification; one finalizer, including created-ref/failed-checkout Partial. |
| Same: branch/tag creation, rename/delete, upstream; pull/push/branch/tag/remote-delete/force-lease | Confirmed target; T; plan + family ref/lease/remote checks; family result | Existing snapshot classification; full recovery refs where supported. Local snapshots do not roll back remote writes. One finalizer. |
| Same: stash push/apply/pop/drop; dedicated `run_recorded_stash` | ADR-0176 approval; T; ordered OID identity + state; mandatory stash verification | Apply/pop existing snapshot rules; drop's own OID is recovery, no auto snapshot. One receipt. |
| Same: discard, restore-snapshot, apply-suggestion | Confirmed paths/content; T; family digest/path/anchor checks; family verification | Discard backups and restore's mandatory savepoint remain independent of the optional toggle. One finalizer. |
| Same: create/open worktree | Confirmed path/branch/config; T; config SHA and family path checks; lifecycle verification | Config approval is separate from owner trust; post-create failure remains visible. One finalizer. |
| `run_recorded_remove` | ADR-0175 approval; management + target T; admin identity/config/dirty preflight; explicit observed verification | Target backups and full branch OID, not management auto snapshot; one receipt, Partial/Unknown retained. |
| `run_history_move` | Confirmed history plan; T; branch/from/to OID; ref-move verification | Ref-only: full from/to + reflog, no worktree snapshot; existing Backend recorder. |
| `execute_lock/unlock/prune/repair_worktrees` | Confirmed plan; T; lifecycle preflight/verification | Administrative metadata, no worktree snapshot; existing lifecycle record boundary. |
| conflict continue/save/abort/skip, stash-conflict abort, stage resolution, directory/file resolution | Existing explicit plan/edit intent; T; existing conflict/session/path/buffer checks | Existing conflict ODB/ORIG_HEAD safeguards and recorder. Full conflict-family approval generation binding remains #484 work. |
| `execute_delete_merged_branches` | Confirmed target list; T; per-ref checks and partial result | Deleted OIDs; existing Backend recorder. |
| `execute_absorb` | Confirmed AbsorbPlan; T; `preflight_absorb`; mandatory `verify_absorb` before success | Common optional destructive savepoint, before rewrite and after preflight. Exactly one Backend record; verification failure after mutation is Partial. |
| `stage_file(s)` / `unstage_file(s)` | Explicit index request; T; path/index conditions in staging | Worktree/refs unchanged: no auto snapshot; one record is the target contract; the existing unlogged implementation remains a staging-family migration gap, not a permanent exception. Admission remains ADR-0175. |
| `fetch_remote` / `fetch_remote_branch` | Explicit request or auto-fetch policy; T; remote/refspec checks | No worktree snapshot; existing fetch recording/admission. Process termination uncertainty remains a distinct result. |
| `create_snapshot` / `prune_snapshots` / `delete_snapshot` | Explicit capture/ID/cap request; T (including deletion); snapshot ref operations | No recursive snapshot. Existing manual GUI recorder remains; automatic children belong to parent execution. This does not migrate metadata recording into a second writer. |
| `trust_worktree_config_for_worktree`, `trust::trust_repo` | Explicit identity/config-SHA grant, allowed before owner/config trust exists; grant-specific rechecks | Trust store only; never an ordinary mutation's implicit trust opt-in. |

Transport PR/SSH and editor filesystem capabilities remain the separate contracts
in DESIGN §5.3; they do not acquire permission through this Git policy. Their
recording and completion migrations are outside this change.

## Enforcing the boundary

Run rejects blockers before savepoint or mutation. It re-derives the operation's
family plan requirements so callers cannot omit a required worktree digest or
clear the destructive flag to bypass its safeguards. The existing HEAD/stash
identity checks remain mandatory. Family plans and domain data are not proof of
user approval; adapters still own explicit approval and one-shot tokens.

Unused external `Backend::execute_*` facades become crate-private: application
consumers must use run or a documented dedicated boundary. Low-level `ops::*`
functions taking an already-open git2 repository remain primitive APIs used by
Git-internal composition and fixtures, not an alternative authorization facade.
No new feature may add a public raw executor instead of extending an existing
boundary; its requirements must have a deliberate row here.

Absorb refuses untrusted and stale plans before making a savepoint. Success
requires the actual HEAD to equal the rebuilt tip; errors still reach its one
recording boundary. Snapshot deletion now rejects untrusted handles before any
ref/index/worktree change. Optional automatic savepoint failure remains
best-effort under ADR-0154; required restore/discard recovery is unchanged.

## Evidence

`crates/kagi-git/tests/backend_policy_test.rs` drives actual fixtures without a
window: policy ON/OFF through direct and worker reset/amend/rebase; CLI/MCP actor
and default snapshot policy despite GUI settings OFF; untrusted delete/absorb
with ref/index/worktree equality; absorb tip verification and stale refusal;
and omitted plan safety requirements refused before any savepoint. Additional G
coverage keeps restore recovery enabled with optional snapshots OFF, changes
policy on one worker, and refuses dirty checkout before creating a branch.
The private, test-only absorb fault proves a mismatched reported tip fails
verification and returns this invocation’s Partial recording receipt.
GUI execution is prohibited for this work; button/Enter parity is established
by their shared execution factory and existing adapter routing, not a new claim
of manual GUI evidence.
