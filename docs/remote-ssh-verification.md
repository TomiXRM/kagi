# Remote-over-SSH (ADR-0089) — local GUI verification checklist

This feature was built and verified **headlessly** (unit tests + a live SSH
integration test against a loopback `sshd`), but the sandbox has no display, so
the **GUI** paths below need a human at a real screen. Run these on a machine
with a working `ssh` client and at least one reachable SSH host that has `git`
installed.

Branch: `claude/remote-git-ssh-client-eb5jox`.

## 0. Setup (once)

- [ ] A remote host you can already `ssh` into **non-interactively** (key auth in
      `~/.ssh/`, the host key already in `~/.ssh/known_hosts`). Confirm with:
      `ssh <host> true` — it must succeed with **no prompt**. Kagi uses
      `BatchMode=yes`, so a host that would prompt (unknown key / password-only)
      will fail by design.
- [ ] At least one git repository on that host, with a few commits and a branch.
- [ ] Build & run Kagi from the branch: `cargo run --bin kagi` (or the bundled
      app). `ssh` and the remote `git` must be on `PATH`.

## 1. Connect dialog (Phase 1)

- [ ] **File → Connect to Remote Host…** opens the dialog (no repo needs to be
      open; the menu item is always enabled).
- [ ] Enter a valid `user@host` (or a `~/.ssh/config` alias) → **Connect**.
      The button shows a spinner, then the dialog switches to the directory
      browser. *(Optional: set a custom Port / Identity file.)*
- [ ] **Error cases** show a readable message and do **not** hang:
  - [ ] Unknown host / typo → "Could not resolve hostname …".
  - [ ] Right host, closed port (e.g. set Port to one nothing listens on) →
        "Connection refused" within ~10 s.
  - [ ] A host not in `known_hosts` → "Host key verification failed" (then add it
        out-of-band with a terminal `ssh` and retry).
- [ ] **Esc** closes the dialog; focus returns to the app (Cmd/Ctrl shortcuts
      still work).

## 2. Remote directory browser (Phase 1)

- [ ] Opens at the remote **home** directory; entries are listed with 📁 folders
      (clickable) and 📄 files (dimmed).
- [ ] Click a folder to descend; the **↑ ..** row goes to the parent.
- [ ] Navigate into a git repository directory → a **● Git repository** card
      appears showing the branch, short SHA, and last commit subject.
- [ ] **Change host** returns to the connection form.
- [ ] A path with spaces in it navigates correctly (shell-quoting sanity).

## 3. Open the remote repo in the graph (Phase 2 / 2b)

- [ ] In a repo directory, click **Open repository (read-only)**. The modal
      closes and the **main graph** renders the remote repo:
  - [ ] Commit graph/lanes draw; the status bar reads
        `Remote (read-only) — user@host:/path`.
  - [ ] Sidebar shows the remote **local branches / remote branches / tags**.
  - [ ] The toolbar/header shows the repo name + current HEAD branch.
- [ ] A **tab appears** for the remote repo, marked with a ☁ cloud glyph.
- [ ] Do this with **no local repo open** (graph shows instead of the Welcome
      screen) **and** with a local repo already open (a new ☁ tab is added).
- [ ] **Tab switching works:** with both a local tab and a ☁ remote tab open,
      click back and forth — the remote tab restores its graph instantly (no
      re-fetch), the local tab restores its normal repo. Opening the same remote
      repo again reuses its existing tab rather than adding a duplicate.
- [ ] Closing the ☁ tab (×) removes it; closing the last tab returns to Welcome.

## 4. Commit inspection over SSH (Phase 2c)

> Commit **metadata** (author / date / message / SHA) comes straight from the
> already-loaded snapshot — it shows **immediately, with no SSH and no server**.
> Only the **changed-files list and the file diff** need an SSH round-trip
> (they're loaded off-thread). No "kagi server" is required for any of this; a
> resident helper would only cut latency, not enable the feature.

- [ ] Click a commit row → the detail panel shows author/date/message **and**,
      after a short SSH round-trip, the **changed-files** list populates.
- [ ] Click a changed file → the full-width **diff view** opens with hunks and
      +/− lines, syntax-highlighted.
- [ ] Try an **Added**, a **Modified**, and (if present) a **Renamed** file —
      each renders sensibly. A binary file shows as binary (no hunks).
- [ ] Keyboard-navigating commits (↑/↓) also loads each commit's changed files
      (the render-trigger covers non-click selection too).

## 5. Working-tree status (Phase 2c)

- [ ] On the remote, make the repo dirty (`echo x >> a_file; touch new_file`).
- [ ] In Kagi press **Cmd/Ctrl-R** (Refresh) → the graph re-snapshots over SSH and
      the **uncommitted-changes row** at the top reflects modified/untracked
      counts. Revert the change on the remote and Refresh again → row clears.

## 6. Read-only gating (Phase 2b)

While a remote repo is shown, confirm these are **disabled / no-op** (the feature
is read-only — nothing should mutate the remote):

- [ ] Menu **Repository → Fetch/Pull/Push** and **Branch → New/Checkout/Delete**
      are greyed out.
- [ ] Double-clicking a branch / pressing Enter on a commit does **not** start a
      checkout. Commit/amend/stash actions are unavailable.
- [ ] No file-watcher activity or auto-fetch fires for the remote (it has no
      local path).

## 7. Leaving the remote view

- [ ] Open a **local** repo (File → Open Repository, or click an existing local
      tab). The remote view is cleared and the local repo behaves normally
      (operations re-enabled, watcher armed).

## 8. Re-run the automated live test (optional, no GUI)

Point the opt-in integration test at your host to re-confirm the non-GUI layers:

```sh
KAGI_REMOTE_TEST_HOST=<user@host> \
KAGI_REMOTE_TEST_PORT=<port-if-not-22> \
KAGI_REMOTE_TEST_REPO=/abs/path/to/repo/on/host \
KAGI_REMOTE_TEST_DIR=/abs/path/to/a/NON-repo/dir \
  cargo test --test remote_ssh_live_test -- --nocapture
```

Expect `[ok]` lines for check_connection, home_dir, list_dir, probe_repo,
repo_summary, remote_snapshot (commits/branches/tags/HEAD), changed_files,
file_diff, and status. Without the env vars the test prints `skipping…` and
passes (CI stays green).

## Known limitations (by design, this milestone)

- Read-only: no remote writes yet (those will go through the OperationController
  pipeline in a later phase).
- Per-file diffstat **bars** are not fetched for remote commits (the diff view
  itself still shows +/− counts).
- Each read is its own SSH round-trip (no resident helper yet), so a big repo's
  first open and per-commit diffs have visible latency on a slow link.
- The internal read path still dispatches on `repo_path == None` rather than a
  formal `GitBackend` trait (no behavioural difference; a later refactor).
