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

## Amendment: #1014 single-session approved pull

The layering and safety sections above describe the original Phase 3 slice.
The current pull is owned by `src/remote/mod.rs` and
`src/app/remote_pull.rs`: planning freezes the SSH route, repository common
directory, physical worktree, branch, HEAD, upstream, branch remote / merge
configuration, remote URL, fetch refspecs, and SHA-256 hashes of the staged
index and porcelain-v2 worktree status. The approved job carries those values
unchanged; execution checks `ssh -G` locally without opening a remote session,
then opens **one** remote SSH session for both preflight and pull.

Inside its remote POSIX `sh` script, Kagi sets `GIT_OPTIONAL_LOCKS=0`,
re-resolves the physical worktree and Git common directory, reads HEAD /
upstream / pull configuration, and compares all frozen values. It resolves the
scratch directory physically and **refuses before creating a file** if it is
inside the worktree or Git common directory. Otherwise a temporary file
outside the repository holds binary fetch / index / status data for exact
comparison; the file is removed before clearing `GIT_OPTIONAL_LOCKS` and
emitting the checked marker. Only then does the same shell
`exec git -c "branch.$branch.mergeOptions=" -C "$top" pull --no-rebase --ff --no-autostash --no-recurse-submodules`.
Before that `exec`, the repository has only been read; no local repository
write or host repository write is initiated. The outside-repository scratch
file is ephemeral, not a claim of zero host filesystem I/O.

Mismatch, unreadable state, or an unsafe scratch location produces a reasoned
pre-exec `Refused` oplog entry and never invokes pull. Known SSH authentication
failure remains `Failed`; a lost session or unclassified output that cannot
prove the pull never started remains `Unknown` (with the existing lease /
reconcile requirement), and a merge stopped mid-way remains `Partial`. There
is no fallback to a second SSH connection. Windows clients also invoke the
system `ssh`; the script runs in the **remote** POSIX shell, so no Kagi-owned
local Unix ControlMaster socket is needed. This closes the gap between two
SSH connections, not concurrent mutation by another process on the host
after the last check and before `exec`.
