# ADR-0098: Remote pull over SSH (ADR-0089 Phase 3)

- Status: Accepted
- Date: 2026-06-17
- Context: ADR-0097 added the first remote write (stash drop). The natural next
  remote operation is **pull** — keeping a remote dev box's branch current from
  its own `origin`. The local pull (ADR-0009/ops) fetches via the system `git`
  binary, then fast-forwards or does an in-memory git2 merge. That git2 merge
  path cannot be replicated over SSH, and there is no local `Repository` in the
  remote view.

## Decision

Implement remote pull by running **`git -C <repo> pull` on the host** over the
system-ssh transport, instead of reproducing the fetch+merge logic locally.

### Layering (mirrors ADR-0097)
1. **I/O** (`src/remote/mod.rs::remote_pull(host, repo)`): runs `git -C <repo>
   pull` via `run_ssh`. The pull executes *on the host*, so the host's own
   credentials, network and config reach its `origin` — Kagi only carries the
   command. Returns git's last summary line (`Fast-forward`, `Already up to
   date.`, merge text) on success; a non-zero exit (no upstream, auth failure,
   or a merge conflict left mid-merge on the host) becomes a `RemoteError`.
2. **Plan** (`src/git/ops/pull_push.rs::plan_pull_remote(branch, upstream,
   behind, ahead, dirty, head)`): synthesises the confirm `OperationPlan` from
   the snapshot's ahead/behind counts (no git2 dry run). Warns when the branch
   has diverged (`ahead>0 && behind>0` → a merge commit) or the remote tree is
   dirty. Non-destructive.
3. **UI** (`src/ui/operations/pull_push.rs`): `open_pull_modal` / `start_pull`
   branch on `remote_view`. `behind == 0` shows the "already up to date" snackbar
   (no modal), matching local. Otherwise the standard pull confirm modal opens;
   on confirm `remote_pull` runs off the UI thread, the oplog records the op
   (synthetic `<host>:<root>` key), and `refresh_remote_view` re-snapshots.

### Safety / scope
- Pull is non-destructive and follows the same plan → confirm → execute → oplog
  path as every other operation. `BatchMode=yes` keeps ssh from hanging/prompting.
- A pull that produces a **merge conflict** leaves the *host* mid-merge and is
  surfaced as an error; resolving a remote conflict from Kagi is out of scope for
  this slice (the user resolves on the host). Fast-forward and clean-merge pulls
  complete fully.

## Consequences
- Remote view now supports pull in addition to stash drop. Push and other writes
  remain unimplemented; they should follow this pattern.
- Verified live: a behind-by-N remote clone (over SSH) pulled to up-to-date via
  the Pull button → confirm modal → fast-forward on the host → view refreshed
  (↓3 → ↓0). IO and plan have unit tests; the FF path was exercised end-to-end.
