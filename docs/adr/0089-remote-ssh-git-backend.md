# ADR-0089: Remote repositories over SSH (agentless, system-`ssh` transport)

- Status: Proposed / Date: 2026-06-16
- Context: Kagi inspects and operates on a Git repository on the **local**
  filesystem via `git2`/libgit2 (`src/git/backend.rs`). The modern AI-coding
  workflow, though, increasingly lives on a **remote** machine: you keep a beefy
  dev box (or a cloud VM) reachable over SSH, run agents/builds there, and want
  to *see and drive its Git state* from a GUI on your laptop. Today people do
  this by `ssh`-ing in and either running a TUI or opening VS Code's
  Remote-SSH. We want Kagi to offer "connect to a host → browse its directories
  → pick a repo → view its repo internals" as a first-class feature, **without**
  betraying Kagi's thesis (predict → confirm → execute; nothing destructive or
  silent) and **without** taking on a heavyweight crypto/SSH dependency.

  Decision (with the user): mirror **how VS Code's Remote-SSH bootstraps** —
  connect through the **system `ssh` binary** (so `~/.ssh/config`, keys,
  `ssh-agent`, and `known_hosts` all "just work", and Kagi implements no auth) —
  but **do not** deploy a resident server in the MVP. Where VS Code pushes a
  `vscode-server` to the host and multiplexes a protocol over the SSH channel,
  Kagi's MVP is **agentless**: it runs short, read-only commands over `ssh`
  (`true`, `pwd`, `ls`, `git rev-parse`, `git log`) and parses their output.
  This is the exact extension of the precedent already in the tree: network git
  (fetch/push) abandoned libgit2 for the **system `git` binary** in `git/cli.rs`
  (ADR-0009) precisely so auth is the OS's job. Remote = run the *system `ssh`*
  the same disciplined way.

## How VS Code does it (the reference we are matching, and where we diverge)

VS Code Remote-SSH is a **two-process split**: the Electron UI stays local and
renders; a `vscode-server` (the real file-I/O, Git, language servers, extension
hosts) is **transferred to and runs on the remote**. Bootstrap uses the system
`ssh` binary (honouring `~/.ssh/config`/agent/`known_hosts`), detects the remote
arch, downloads/extracts the matching server under `~/.vscode-server/`, and then
multiplexes a custom protocol over that one SSH connection. "Browse the remote
directory tree" is therefore an **RPC to the remote server**, not the laptop
reading the remote FS.

Kagi keeps the **transport** (system `ssh`, OS-owned auth) and drops the
**server** for the MVP. Trade-off: no resident process to install/update/secure,
at the cost of one SSH round-trip per probe (acceptable for read-only browsing).
A resident helper is the natural later step (see *Future*).

## Decision

Add a **remote-over-SSH read path** layered the same way as every other Kagi
feature (ADR-0072 pure domain + thin I/O):

### Layering (no `git2`/process in the view; no I/O in the domain)
1. **domain** (`crates/kagi-domain/src/remote.rs`, pure, std-only): `RemoteHost`
   (`[user@]host[:port]` parse, ssh-config alias passthrough), **argv
   construction** (`ssh_invocation` → the exact `Vec<String>` after the program
   name, connection options before the destination so `remote_tokens` can never
   be read as ssh options), POSIX `shell_quote`/`join_remote_command`,
   `join_path`/`parent_dir`, and **output parsers** (`parse_ls`,
   `parse_repo_probe`, `parse_repo_summary`). Unit-testable from strings — no
   host, no key, no network.
2. **app/io** (`src/remote/mod.rs`, the *only* layer that spawns `ssh`):
   `check_connection`, `home_dir`, `list_dir`, `probe_repo`, `repo_summary`.
   Same hardening as `git/cli.rs` — argv array (no shell interpolation locally),
   `LC_ALL=C`, `BatchMode=yes` (the SSH analogue of `GIT_TERMINAL_PROMPT=0`), a
   `stdin=null`, and a background-thread whole-command timeout.
3. **ui** (later phase): a "Connect to host…" entry → connection dialog → remote
   directory picker (navigates with `list_dir`, marks repos via `probe_repo`) →
   on pick, a detail panel showing `repo_summary`. Not in this slice.

### Security & non-interactivity (Kagi's "never hang, never silently accept")
- **`BatchMode=yes`** on every invocation: ssh never prompts, so a missing
  `known_hosts` entry or a password-only host **fails fast** with a clear error
  instead of hanging the UI. Kagi **never auto-accepts a new host key**
  (no `StrictHostKeyChecking=accept-new`) — the user records the key once
  out-of-band (a normal terminal `ssh`), consistent with the safety thesis.
- **`ConnectTimeout`** bounds the handshake; a separate whole-command timeout in
  the I/O layer bounds a hung remote command.
- **Local shell-bypass** (argv array) + **remote shell-quoting** (`shell_quote`)
  together mean a repo path like `/srv/my repo; rm -rf ~` is transmitted as one
  literal argument on both sides — no injection on either end. This is covered
  by domain unit tests.
- **Read-only.** This slice exposes inspection only. Remote *writes* (checkout,
  commit, push, …) are explicitly out of scope and, when added, MUST flow
  through the existing `OperationController` plan→confirm→execute→verify
  pipeline (ADR-0073), never directly from `src/remote`.

### Repo detection nuance
`probe_repo` distinguishes a **transport failure** (unreachable host, auth
denied, host-key failure → surfaced as `RemoteError`) from `git` running and
answering **"not a repository"** (a definitive negative → `RepoProbe::not_a_repo`,
*not* an error), via an ssh-failure-marker heuristic on stderr. So the directory
picker can grey-out non-repo dirs without treating them as connection errors.

## Alternatives considered
- **Pure-Rust SSH (`russh`)** — full control and no external `ssh` dependency,
  but re-implements auth, `~/.ssh/config`, and `known_hosts`, pulls a large
  crypto tree, and diverges from the OS-owns-auth precedent (ADR-0009).
  Rejected for the MVP.
- **libgit2 SSH transport (`ssh2`/libssh2)** — only speaks the git wire protocol
  (clone/fetch/push); it **cannot** run arbitrary remote commands, so it cannot
  satisfy the core requirement ("browse the remote host's directories and detect
  repos"). Rejected.
- **Mount the remote FS (sshfs/NFS)** — then reuse the local `git2` backend
  unchanged. Tempting, but needs a mount step outside Kagi, has poor latency for
  libgit2's many small reads, and is platform-uneven. Rejected for the MVP;
  could return as an opt-in.
- **Deploy a Kagi helper agent now (the full VS Code model)** — best latency
  (one persistent connection, no per-probe handshake) and the eventual target,
  but it means shipping, versioning, and securing a remote binary. Deferred.

## Phased rollout
- **Phase 0 — read-only foundation (this slice):** `kagi-domain::remote` +
  `src/remote/` — connect-check, home dir, directory listing, repo detection,
  HEAD summary. Pure parsers + argv are unit-tested in CI (no host needed); the
  `ssh`-spawning functions are covered by manual testing against a real host.
- **Phase 1 — UI (done):** connection dialog + remote directory picker + remote
  repo detail panel (branch, HEAD, last commit). Still read-only.
- **Phase 2 — remote read snapshot (landed: engine + log preview):**
  `kagi::remote::remote_snapshot()` builds the *same* `RepoSnapshot` the local
  `git2` backend produces — HEAD, commits (topo-order, parents), local branches
  (+ upstream ahead/behind), remote branches, tags, stashes — by running `git`
  reads over SSH and parsing them with pure, unit-tested
  `kagi_domain::remote_snapshot` parsers (separator-framed `git` formats).
  Working-tree `status` and full `worktrees` are left minimal (a read-only view
  does not need a porcelain parse yet).
- **Phase 2b — render the remote repo in the main views (done):** the remote
  browser's "Open repository (read-only)" feeds the snapshot through
  `build_tab_view`/`apply_tab_view` into the **real** graph/sidebar/detail views
  via `KagiApp::enter_remote_view`. The gating is structural, not a thicket of
  guards: a remote view sets `repo_path = None`, so every operation (already
  written as `let p = self.repo_path.as_ref()?；`) and the fs watcher
  (`arm_watcher` early-returns on `None`) disable themselves, and `command_state`
  greys out the write commands (no `has_repo`). A remote repo gets a **real tab**
  (a `RepoTab` carrying a `remote` marker + a synthetic `<host>:<root>` identity
  path, its view cached for instant tab-switching) so it appears in the strip and
  can be switched to/from like any repo; a `remote_view` marker drives the
  read-only UI and keeps the workspace visible. `Refresh` (cmd-r) re-points at a
  re-snapshot over SSH. Switching to a local tab clears the remote view.
- **Phase 2c — remote diffs + working-tree status (done):** selecting a commit
  in the remote graph loads its changed files (`git diff-tree --name-status -M`)
  and clicking a file loads its unified diff (`git show -- <path>`), both **off
  the UI thread** (the selection path triggers a one-shot async load guarded by a
  `remote_diff_inflight` set; the file-diff path spawns on click) and rendered by
  the same `MainDiffView` as local (shared `set_commit_main_diff`). Parsing is
  pure + unit-tested in `kagi_domain::remote_diff` (name-status incl. renames;
  unified-diff hunks with old/new line-number tracking; binary). `remote_snapshot`
  now also fills working-tree `status` from a porcelain-v1 parse, so the
  uncommitted-changes row reflects the remote tree. **Remaining:** a formal
  `GitBackend` trait (`LocalBackend` git2 / `RemoteSshBackend` ssh+CLI) to replace
  the `repo_path == None` dispatch — an internal refactor with no behaviour change,
  deliberately deferred; per-file diffstat bars over SSH; and a resident helper to
  collapse the per-read SSH round-trips (the VS Code-server analogue).
- **Phase 3 — remote operations:** writes via the `OperationController`
  pipeline; later, an optional resident helper for latency (the VS Code-server
  analogue) and connection multiplexing.

## Consequences
- New, isolated modules only (`kagi-domain::remote`, `src/remote/`). No existing
  code path changes; the local `git2` backend is untouched. The domain stays
  pure (std-only) and the view stays process/network-free.
- New external dependency at runtime: an `ssh` client on the user's machine
  (already required for git-over-SSH, so effectively free) and `git` on the
  remote.
- Consistent with the safety thesis: inspection is read-only; connections never
  hang and never silently trust a new host; remote *writes* remain gated on the
  predict→confirm→execute pipeline when they land.
