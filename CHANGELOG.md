# Changelog

All notable changes to Kagi are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [0.3.12] — 2026-06-16

### Added
- **Background progress is a single, unified snackbar.** Every slow operation
  (merge, pull, push, stash, checkout, commit, …) runs off the UI thread and
  shows one busy snackbar with a large spinning sync icon + label
  ("Merging…", "Pulling…"). The old per-operation "X: started" toasts are gone
  (ADR-0086).

### Changed
- **No-op Push / Pull no longer pops a dialog.** When there's nothing to push
  or pull (already up to date), Kagi shows a quick "Already up to date"
  snackbar with the same big sync icon instead of opening a confirmation modal.
  Real push/pull operations still show the confirm modal (ADR-0086).
- **Merge no longer freezes the window.** Merge planning and execution run on a
  background thread; the sync icon spins while busy. The merge confirm button
  is now just "Merge" (it could overflow the window with long branch names).

### Fixed
- **Add/add text conflicts show in the conflict editor.** Files added on both
  sides (no common ancestor) — e.g. `.h` headers — were misdetected as binary
  and hidden; they now materialize as a 3-way text conflict.
- **Terminal loads your shell config and scrolls.** The embedded terminal now
  starts a login + interactive shell (so `~/.zshrc`/PATH apply — `python` etc.
  resolve) and vertical scrollback works.
- **Discard handles untracked files.** "Discard all" now includes untracked
  files (deletes them, backed up to the oplog first) and prunes now-empty
  folders — equivalent to `git clean -fd` but recoverable (ADR-0083).

### Performance
- **Branch / tag / remote sidebar is virtualized** (uniform_list), so scrolling
  and terminal typing stay smooth on large repositories.

## [0.3.11] — 2026-06-15

### Fixed
- **Fonts render consistently on Linux.** Kagi now bundles **Inter** (UI) and
  **JetBrains Mono** (terminal / conflict editor / code) and loads them at
  startup, instead of relying on the platform default and the macOS-only "Menlo"
  fallback (which rendered broken on Ubuntu). The look is now identical on every
  OS; CJK still falls back to a system font.
- **Window no longer opens off-screen.** The initial size is the preferred
  1440×920 but clamped to the active display (≤92% width / 90% height, 900×600
  floor), so it always fits on small / scaled displays.

### Changed
- Polished theme colors for secondary controls and the title bar.

### Docs / internal
- Documented the Linux build dependencies (apt packages) for building from
  source. Silenced macOS dead-code warnings for the Linux-only in-app menu.

## [0.3.10] — 2026-06-15

### Fixed
- **Linux AppImage installer now works with no arguments.** The zip nested the
  install script under `scripts/` while the AppImage and icon sat at the root, so
  `install_linux_desktop.sh` couldn't find them and only printed its usage
  message. The zip is now **flat** (script next to the AppImage + icon, per
  ADR-0047), and the script's auto-detect also searches the unzip root — so
  `unzip … && bash install_linux_desktop.sh` registers Kagi under `~/.local`.

## [0.3.9] — 2026-06-15

### Added
- **Checkout a remote-only branch from the commit graph.** Right-clicking a
  commit that carries a remote-only badge (e.g. `origin/feature` with no local
  branch) now offers **"Checkout '<remote>' as local branch…"** — it creates a
  local tracking branch and switches to it (the same flow as the sidebar). It is
  hidden when a local branch of that name already exists.

### Changed
- **Enter approves / Esc cancels the active modal.** When any confirmation/plan
  modal is open, Enter confirms it and Esc cancels it.
- **Taller title bar** for a bit more padding around the tabs and traffic lights.

## [0.3.8] — 2026-06-15

### Added
- **Cmd+Z / Cmd+Shift+Z for Undo / Redo** of git operations (ADR-0084). Bound so
  they never shadow text-input undo (in the commit message box) or the
  integrated terminal's Cmd+Z — they only act on the commit graph.
- **Undo works on a freshly-opened repository.** The undo/redo history is now
  seeded from the current branch's **reflog** on open, so you can undo the last
  operation(s) even in a repo you just opened (not only ones done this session).
  Switching tabs re-seeds from the new repo's reflog.

### Changed
- **Undo of a commit now uses `git reset --soft` semantics** — the undone
  commit's changes come back **staged** (index untouched, working tree
  preserved), instead of unstaged. Still a safe ref-only move: no `reset --hard`,
  no `clean`, and the commit stays in the object store + reflog.

## [0.3.7] — 2026-06-15

### Added
- **Drag-and-drop merge of upstream-only branches.** A remote-tracking branch
  with no local counterpart (e.g. `origin/feature`) can now be dragged — from a
  commit-graph remote badge or the sidebar remotes list — onto the current branch
  to merge it directly via its remote ref (no local branch is created).
- **Background auto-fetch.** Kagi now periodically fetches the remote (every few
  minutes, while a repo is open) so the commit graph and ahead/behind counts stay
  current without manual fetches. New **Settings ▸ Appearance ▸ Auto-fetch**
  toggle (on by default).

### Changed
- **The 🔁 Refresh button now also fetches** the remote in the background (so a
  merge done on GitHub shows up). It re-reads local state instantly and pulls the
  remote quietly — failures (offline / no remote) are silent.
- **Pull and Push are no longer grayed out** when there's "nothing" to do. Pull is
  enabled whenever the branch has an upstream; Push whenever a remote exists. The
  old ahead/behind gating used possibly-stale counts and caused a "can't pull
  after a remote merge" dead-end. A no-op pull/push is harmless.

### Fixed
- **"Discard all" now removes newly-added (untracked) files** too, instead of
  leaving them. Untracked files are deleted from disk after their content is backed
  up to the ODB (recorded in the oplog) — recoverable with `git cat-file -p <sha>`,
  exactly like a tracked discard. This is not `git clean` (ADR-0083). Per-file
  Discard is also offered on untracked rows now.

## [0.3.6] — 2026-06-15

### Added
- **Two new themes: Tokyo Night and IBM PC.** Tokyo Night is a navy/blue-green
  dark theme; IBM PC is a black-background CGA 16-colour theme.
- **Smart commit message: the Suggest button now uses the local LLM when one is
  available.** When Ollama is enabled, *Suggest* sends the staged diff to the
  LLM and uses its output (button turns green); otherwise it falls back to the
  rule-based suggestion (blue). The separate "Generate with Local LLM" button is
  gone — folded into Suggest.
- **Snackbar slide animation.** Toasts slide in from the left (fade in) when they
  appear and slide back out (fade out) when they expire or are dismissed.

### Changed
- **Themed window title bar.** The title bar is no longer the default OS gray —
  it is transparent so kagi's themed top bar (the repo tab strip) fills the
  title-bar area and follows the active theme. The strip is draggable and, on
  macOS, leaves room for the traffic lights.
- **Ctrl+A selects all in text inputs** (e.g. the commit message), instead of
  jumping to line start. Double-click word-select and ⌘A already worked.

### Fixed
- **Line-level conflict merge interleaves by position.** When taking individual
  lines from both sides of a hunk, the result now keeps each line in its
  original position instead of grouping all of one side first — so "base on the
  left, pull in just line 10 from the right" lands line 10 in place.
- Commit panel: hid the scrollbar on the stage/unstage lists (they still scroll),
  and added a left margin before the per-row Stage/Unstage buttons.

## [0.3.5] — 2026-06-15

### Performance
- **Commit panel no longer janks the whole UI.** It used to run a full
  `working_tree_status` every render frame (for the staged preview) and read every
  untracked file for a diffstat — so opening it on a large repo dropped the app to
  ~6fps and a bulk untracked drop (e.g. 300 images) froze it. Now the preview is
  cached, untracked files are not diffstatted, and the file lists are **virtualized**
  (`uniform_list`, O(visible) per frame) — scrolling stays smooth with hundreds of
  changes.

### Added
- **WIP auto-refreshes on working-tree changes**, not only on git operations: the
  watcher now watches the working tree and refreshes the WIP / commit panel when
  files change on disk (background status check; a no-op when nothing the repo
  cares about changed, so a busy nested worktree doesn't cause reload storms).
- **Persisted commit-list column widths** (BRANCH/TAG, GRAPH) — your resize sticks
  across restarts.

### Fixed
- Watcher no longer reloads this view on **sibling worktree / submodule** git
  activity (`.git/worktrees/…`, `.git/modules/…`) — fixes the reload storm from an
  active Claude Code worktree.
- Nested git worktrees/repos are no longer listed as a giant "untracked" entry in
  the commit panel.
- Header: a long repo/branch label no longer overlaps the Pull/Push/Branch
  buttons — the repo name now sits above a smaller current-branch line, each
  truncating with an ellipsis.
- Commit panel: the per-file Stage button is right-aligned again.

## [0.3.4] — 2026-06-14

### Added
- **In-app auto-update** (ADR-0082). On startup Kagi checks GitHub Releases in the
  background (best-effort, silent on failure, opt-out via Settings) and shows an
  **"↑ Update vX.Y.Z"** chip in the header when a newer release exists. Clicking it
  opens a modal with the current → latest versions, the platform asset, and the
  **release notes rendered as Markdown**. "Update now" downloads the asset,
  **verifies its SHA-256** against the release checksums, swaps it into the running
  install atomically, and relaunches — or "Skip this version" / "Release page" /
  "Later". Checking is opt-in and silent; installing is always confirmed and
  checksum-verified, writes atomically, and runs no destructive command.
  - Linux installs cleanly; **macOS/Windows are unsigned**, so the OS still warns
    on the relaunched build until code signing lands (ADR-0038 Phase 2). The macOS
    path is verified end-to-end; Linux/Windows install paths are implemented but not
    yet runtime-verified by the maintainers.

## [0.3.3] — 2026-06-14

### Added
- **Windows build** (x86_64), experimental / best-effort. Releases now ship
  `kagi-<version>-x86_64-windows.zip` (a self-contained `kagi.exe` — assets are
  embedded). The terminal uses `cmd.exe` and settings/avatars/oplog resolve under
  `%USERPROFILE%`. Built and packaged by CI; not yet runtime-verified by the
  maintainers, and unsigned (SmartScreen warns on first launch).

### Fixed
- **Conflict editor, mismatched-length sides.** Scrolling the longer of the two
  panes was clamped to the shorter side's line count (the panes share one scroll
  handle but had unequal row counts); each hunk now blank-pads the shorter side so
  both panes have equal height.
- **Conflict editor, missing context.** The A/B panes skipped non-conflicting
  context lines, so the Merged Result Preview contained lines that were invisible
  in the editor (reading as code at "unexpected positions"). Context lines now
  render on both panes (muted, with real per-side line numbers) and stay aligned,
  so each pane shows the full file and the preview is traceable to what's on screen.

## [0.3.1] — 2026-06-14

### Fixed
- **Could not commit after resolving a merge conflict.** After resolving all
  conflicts and clicking **Continue**, Kagi advanced to the commit panel but the
  commit could not be completed: the resolutions were never staged (the per-file
  Save is optional), so the index kept its unmerged entries — the Commit button
  stayed disabled and the merge commit was refused. Continue now stages the
  resolutions before opening the commit panel, and a resolved merge (MERGE_HEAD
  present, no remaining unmerged entries) is treated as "ready to commit" rather
  than re-entering an empty Conflict Mode, so the commit panel stays put across
  the filesystem-watcher reload that staging triggers. GUI-verified end to end.

## [0.3.0] — 2026-06-14

This release ships new user-facing features on top of the start of the v1.0
internal re-architecture. See `docs/rearch/` for the architecture work and
`docs/adr/0072`–`0081` for the decisions behind it.

### Added
- **Drag-and-drop branch merge** (ADR-0079, T-DNDMERGE-001). Drag a local-branch
  label — from the commit-graph **BRANCH / TAG** badges *or* the sidebar branch list —
  and drop it onto the current branch to **start** a merge. The dropped label follows
  the cursor; each badge is independently draggable (a commit may carry several
  branches). The drop only opens the merge **preview** (`Merge <source> into <current>`
  with current→predicted state, fast-forward vs merge-commit, conflict prediction) —
  nothing is merged until you confirm. Cancel leaves the repository untouched; on
  conflict it enters the existing Conflict Mode.
- **Settings button + window** (ADR-0080, T-SETTINGS-001). A gear button in the
  window's top-right (also ⌘, / menu bar) opens a settings view (sections for
  Appearance — theme, UI zoom, compact graph — and Language: English / 日本語),
  applied live and persisted to `~/.kagi/settings.json`.
- **Undo / Redo of operations** (ADR-0081, T-UNDOREDO-001). GitKraken-style
  Undo/Redo toolbar buttons that work after commit and merge, implemented as safe,
  reflog-backed branch-ref moves through the plan→confirm→preflight→execute→verify
  pipeline — every move shows a preview first, no commit is ever destroyed, and
  `reset --hard` is never used (undone commits stay recoverable via the reflog).
- **`kagi <repo>` CLI** — `cargo install --path .` puts a self-contained `kagi`
  binary on your `PATH`; `kagi <repo-dir>` opens that repo (no arg → Welcome).
- **Smooth commit** — the Commit button commits immediately (no confirmation
  popup) when the pre-commit checklist finds no blockers; blockers (conflict
  markers / secrets / large binaries) still show the safety modal.

### Fixed
- Integrated **terminal arrow keys** (shell history) and **Escape** (vim/less) now
  work — they were being consumed by global diff/close key bindings.
- Settings window: the top-right gear icon now renders (missing bundled SVG), the
  layout/contrast is correct (rebuilt as a native view), the theme selector is a
  dropdown, and opening Settings no longer panics.
- Header toolbar button cluster is now centered (was right-shifted).

### Changed (internal — v1.0 re-architecture groundwork)
- Extracted a pure **`kagi-domain`** crate (commit/graph/diff/conflict model, rules,
  plan types — zero `git2`/`gpui`) (ADR-0072).
- Introduced a **`Backend` façade** + unified **`Operation`** pipeline; the **UI no
  longer calls `git2` directly** (enforced by a CI grep gate) (ADR-0073/0078).
- Began decomposing the 16.7k-line `ui/mod.rs` god-file (modals, diff view extracted)
  and slimming `main.rs` (ADR-0076/0077).
- Added a **test CI** workflow (`cargo test --workspace` + the UI-git2-free gate);
  the suite stays green at every commit.

## [0.2.0]
- Conflict Mode (line-level 3-pane editor, merge-into-conflict), commit suite,
  repo tabs, themes, EN/JA UI, uniform zoom, integrated terminal, GitHub avatars,
  cross-platform distribution. (See the v0.2.0 release notes / git history.)

## [0.1.0]
- Initial release: commit-graph UX, branch/tag/stash/worktree management, staging +
  commit, cherry-pick / revert / amend / discard with dry-run safety.
