# ADR-0097: Remote stash drop over SSH (ADR-0089 Phase 3 — first remote write)

- Status: Accepted
- Date: 2026-06-17
- Context: ADR-0089 added a **read-only** remote-over-SSH view (snapshot, graph,
  diffs) and explicitly deferred remote *writes* to "Phase 3", noting they must
  keep Kagi's safety thesis (plan → confirm → execute → verify/oplog) and the
  destructive-command ban. A user working against a remote dev box (Tailscale,
  Linux) hit the gap: the stash context menu's **Drop** did nothing in the remote
  view, because every write path early-returns when `repo_path` is `None`.

## Decision

Implement **remote stash drop** as the first remote write, mirroring the local
destructive stash-drop (ADR-0087) exactly in UX and discipline, but executing
over the system `ssh` transport instead of git2.

### Layering
1. **I/O** (`src/remote/mod.rs::remote_stash_drop(host, repo, index)`): runs
   `git -C <repo> stash drop "stash@{index}"` via the existing hardened
   `run_checked` (argv array, `BatchMode=yes`, `LC_ALL=C`, whole-command timeout).
   Returns the dropped-entry summary on success, `RemoteError` on non-zero exit
   (e.g. the index no longer exists). Only removes the stash ref — never touches a
   working tree (there is none locally; the remote tree is untouched).
2. **Plan** (`src/git/ops/stash.rs::plan_stash_drop_remote(label, head_summary)`):
   synthesises the danger-confirm `OperationPlan` the modal needs — `destructive:
   true`, an irreversible-action warning, no blockers — without a git2 dry run
   (there is no local `Repository`). Lives in the ops module so it can fill the
   plan's private fields.
3. **UI** (`src/ui/operations/stash.rs`): `open_stash_drop_modal` / `start_stash_drop`
   branch on `remote_view.is_some()`. The remote branch shows the **same**
   `StashDropModal` danger confirmation, then on confirm runs `remote_stash_drop`
   off the UI thread, records the oplog (op `stash-drop`, synthetic
   `<host>:<root>` repo key), and re-snapshots via `refresh_remote_view` so the
   dropped entry and its graph row disappear. One drop at a time (a re-snapshot
   follows each, so `stash@{N}` indices stay correct).

### Safety
- Stash drop is **not** a banned destructive command (`push --force` / `reset
  --hard` / `git clean`); it is the same explicitly-confirmed Destructive op as
  the local path (ADR-0087). The confirmation modal, irreversible warning, and
  oplog entry are all preserved for the remote case.
- `BatchMode=yes` keeps the safety guarantee that ssh never hangs or silently
  accepts a new host key.

## Consequences
- The remote view is no longer strictly read-only: stash drop is a supported
  write. Other remote writes (pop/apply/checkout/commit/push) remain unimplemented
  and should follow this same pattern when added.
- Verified live against a real remote (Tailscale, Linux, git 2.43): right-click
  stash → Drop → confirm → the stash was removed on the host and the view
  refreshed (stash count decremented).

## Not done (follow-ups)
- The remote stash menu still shows **Pop / Apply** (working-tree ops that no-op
  on a read-only remote). They should be hidden or surfaced as unavailable in the
  remote view.
- A formal `GitBackend` trait (`LocalBackend` / `RemoteSshBackend`) to replace the
  `remote_view.is_some()` dispatch (ADR-0089 deferred this).
