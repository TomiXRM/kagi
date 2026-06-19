# Changelog

All notable changes to Kagi are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [0.4.0] — 2026-06-19

### Added
- **Branch Solo focus mode.** Right-click a branch badge in the graph and choose
  **Solo** to dim every commit that isn't in that branch's history; choose **Exit
  Solo** to restore. History is walked via first-parent ancestry. (#47)
- **Branch context menus on graph badges.** Right-clicking a local or remote
  branch badge in the commit graph now opens the branch action menu directly.
- **WIP-row diffstat.** The synthetic working-tree (WIP) row shows an aggregated
  staged + unstaged `+N / -M` count, refreshed from the backend on load/reload.

### Changed
- The WIP-row diffstat is rendered at the **right end** of the row, larger and
  bold, for better legibility.
- Per-file **Stage / Unstage** buttons in the commit panel are slightly smaller
  so they no longer exceed the file-row height.

## [0.3.22] — 2026-06-19

### Fixed
- **Smart Commit now finds the Claude Code / Codex CLIs in the macOS app bundle.**
  A `.app` launched from Finder/Dock doesn't inherit the login shell's `PATH`, so
  CLIs installed via mise / Homebrew / `~/.local/bin` showed as "not on PATH"
  (they worked from a terminal). kagi now resolves the login shell's PATH for
  both detection and execution. (#44)

### Docs
- Added a **File History** section to the README (English + Japanese).

## [0.3.21] — 2026-06-19

### Changed
- **Refreshed the app icon.** Regenerated the macOS `.icns` and Linux PNGs from
  an updated source image. Added `assets/README.md` documenting the icon
  pipeline (`xtask icon` / `scripts/make_icon.sh`).

## [0.3.20] — 2026-06-19

### Added
- **Smart Commit can use the Claude Code / Codex CLIs.** If you have the `claude`
  or `codex` CLI installed and logged in, you can pick it as the commit-message
  provider in Settings (in addition to local Ollama). kagi runs it
  non-interactively and **read-only** (it can never modify the repo) and shows a
  clear warning that your staged diff is sent to that external CLI and consumes
  your own account's usage/quota. Opt-in; off by default. (ADR-0099)
- **Connect to an SSH remote from the Welcome screen.** A *Connect to SSH remote…*
  button sits next to *Open Repository…* when no repo is open.
- **Recent repositories on the Welcome screen.** A list of recently-opened repos
  (name + path); click to reopen. Missing paths are dropped automatically.
- **New colour themes:** Pinky Boo, Catppuccin Latte, and Dracula.

### Fixed
- **In-app update now works for the AppImage build** (issue #29). The updater
  replaces the writable `.AppImage` file itself (download → verify → swap →
  relaunch) instead of bailing out. The tar.gz install path is unchanged.
- **Smart Commit LLM settings.** You can now enable Smart Commit's LLM and pick
  the Ollama model from Settings — the enable toggle was missing and the model
  picker didn't populate unless the commit panel had been opened first.
- **Update dialog is readable for long release notes** — wider (0.8× the window),
  the notes scroll, the markdown is sized down, and it follows the dark theme.

### Changed
- **The theme list is sorted alphabetically** (the default, Catppuccin Mocha,
  stays first) so the Settings picker stays tidy as themes are added.

### Docs
- Added a research note on GitHub pull-request integration (how GitButler/Fork
  do it; recommended approach for kagi) for a future feature.

## [0.3.19] — 2026-06-19

### Added
- **Switch branches without a forced stash.** Branch checkout no longer blocks on
  *any* uncommitted change — it only blocks when your local changes actually
  collide with the target branch (a path that differs between the two and is
  locally modified). Non-conflicting changes are carried over to the target
  branch with a heads-up warning, matching how commit checkout already behaved.

### Fixed
- **Stage/Unstage button colours.** The buttons used gpui-component's filled
  `success`/`warning` variants whose hover/foreground colours kagi never mapped,
  so the white label washed out (and gpui-component 0.5.1 hardcodes the hover
  text colour). They now use a translucent, theme-tinted style that reads like
  the branch-list rows.
- **The commit panel no longer closes when you stage/unstage a file.** Staging
  writes `.git/index`, which the file watcher treated as a graph change and
  triggered a full reload ~0.3 s after the click, closing the panel. Index-only
  changes now do a light in-place refresh that keeps the panel open.
- **Arrow keys now navigate the File History view.** In the per-file history
  view, up/down moved the (hidden) main commit list instead of the history
  entries, so the selection and diff never changed. They now move the history
  selection and update the diff.
- **File History selection highlight.** A hovered row used the selection colour,
  so the row the mouse was left on after a click looked "still selected" while
  the arrows moved the real selection — now hover uses a subtle tint and exactly
  one row reads as selected.

## [0.3.18] — 2026-06-18

### Added
- **Settings theme picker is now a real dropdown** (gpui-component `Select`) with
  keyboard navigation, replacing the hand-rolled inline option list. The On/Off
  toggles (Compact graph, Auto-fetch) are proper `Switch`es and the language
  choice is a `RadioGroup`.

### Fixed
- **Settings rows could overflow the panel.** Wide controls combined with
  unbreakable (CJK) labels pushed the control past the panel's clipped edge,
  hiding it; the label column now shrinks so the control stays inside.
- **Settings could not scroll when zoomed in.** Lower sections (Smart Commit /
  LLM) were clipped and unreachable; the content now scrolls, and the panel is
  sized to a fraction of the window so it always fits.
- **Diff text was hard to read on light themes.** Added/removed line text used a
  fixed light green/red that washed out on the light diff backgrounds; it now
  uses the per-theme colours, readable across all themes.

### Changed (internal)
- **Adopted gpui-component widgets across the UI.** Hand-rolled buttons throughout
  the modals, conflict views, inspector, commit panel, file-history/diff headers,
  and tab strip are now the shared `Button`; the conflict editor's icon button and
  the diff/settings controls follow suit. Reduces bespoke styling and keeps the UI
  consistent with the theme.
- **Unified the commit/branch/stash context menus** into one generic overlay
  renderer (they were three near-identical copies), removing ~260 lines.
- **Sped up debug builds.** The dev profile raises the GPUI rendering/text-shaping/
  layout crates to opt-level 3, so `cargo run` is no longer sluggish during
  development without slowing incremental rebuilds.

## [0.3.17] — 2026-06-17

### Fixed
- **Branch-picker dialog could swallow a row click.** The overlay's clickable
  rows were not occluded, so a mouse-down on a branch propagated to the
  full-screen dismiss scrim beneath it and closed the overlay before the row's
  click completed — selecting a branch silently did nothing. The panel now
  occludes, matching every other menu/modal.

### Changed (internal)
- **Tuned the release build profile** (`lto = "thin"`, `codegen-units = 1`,
  `strip = true`). Kagi's interactive cost is dominated by tree-sitter
  highlighting, git2 diffs and commit-graph layout, so this makes distributed
  release builds faster at runtime and noticeably smaller. (If Kagi ever feels
  sluggish during development, make sure you are running a `--release` build —
  debug builds are 10–50× slower on these paths. See `docs/linux-development.md`.)
- **Added a Linux/Ubuntu development & testing guide** (`docs/linux-development.md`):
  system dependencies, debug-vs-release performance, Wayland/XWayland, Blade/Vulkan
  device selection, the test suite, and bundling.

## [0.3.16] — 2026-06-17

### Added
- **Remote stash drop over SSH.** The stash context-menu **Drop** now works in
  the read-only remote view (ADR-0089 Phase 3): the same danger-confirm modal and
  oplog as local, executing `git stash drop` on the host over the system-`ssh`
  transport, then re-snapshotting (ADR-0097).
- **Remote pull over SSH.** The **Pull** button now works in the remote view —
  `git pull` runs on the host (its own credentials reach its `origin`), so
  fast-forward and clean-merge pulls complete; a conflict is surfaced for
  resolution on the host. Same confirm + oplog discipline as local pull (ADR-0098).

### Fixed
- **Commit detail panel no longer hidden on repos with long commit messages.**
  The center commit-list column had no flex `min-width`, so a long commit/merge
  message could push the right-hand Inspector off-screen (most visible on remote
  dev repos with long branch names): clicking a commit selected it but showed no
  detail. The column now shrinks and truncates so the Inspector keeps its width.
- **Stash graph connection lines were drawn off-screen on wide graphs** (many
  branches). Stash lanes are now packed from the lane count in use near the top
  of history instead of the global maximum, so the stash nodes and their
  connection lines stay visible (ADR-0088).

### Changed (internal)
- **Codebase structural refactor (issue #13).** Added `AGENTS.md`; split the
  `ui/mod.rs` god-file into `types.rs` / `render.rs` / `operations/` and
  `git/ops.rs` into per-op modules; extracted `settings.rs`; introduced an
  `ActiveModal` enum, a `view_models` layer, an `active_view` single source of
  truth, and a `klog!` log-contract macro (ADRs 0091–0096). Behaviour-preserving;
  no user-facing change.

## [0.3.15] — 2026-06-17

### Added
- **Remote repositories over SSH (read-only).** Connect to a host over SSH from
  **File → "Connect to Remote Host…"**, browse its directories, and open a repo
  to inspect its graph/branches/tags/commits and per-commit file diffs — all
  **read-only**. It is **agentless**: nothing is installed on the remote; Kagi
  runs short read-only `git`/`ls` commands over the system `ssh`, so
  `~/.ssh/config`, keys, ssh-agent, and `known_hosts` just work (set new or
  password-only hosts up in a terminal first). Remote views are structurally
  read-only — every write operation and the fs-watcher disable themselves
  (ADR-0089).
- **Per-file commit history.** A new view lists every commit that touched a
  given file, with a resizable list/diff split (ADR-0089).
- **Smart-commit: body generation in template mode** — the model now fills the
  commit body field, plus a model picker in Settings and an OpenCommit-style
  prompt (`think:false` for reasoning models); the Style toggle was dropped
  (ADR-0090).

## [0.3.14] — 2026-06-16

### Added
- **Stashes in the commit graph.** Each stash now appears as a row directly
  below the WIP row, in yellow with a stash (inbox) icon, and draws a branch
  line down to the commit it was created on — so you can see where each stash
  sprouted from, even when its base is an older commit (ADR-0088). Left-click a
  stash row to Pop, right-click for the Pop/Apply/Drop menu.

### Fixed
- The branch/tag (and stash) **label→node connector line now extends into the
  BRANCH/TAG pane** instead of stopping at the column boundary, and runs level
  across the divider (previously a ~1px step).

## [0.3.13] — 2026-06-16

### Changed
- **Stash actions in the sidebar.** Left-clicking a stash now **pops** it
  (apply + remove) instead of applying-and-keeping — so a stash you act on
  actually goes away. Right-click opens a menu with **Pop**, **Apply** (keep),
  and **Drop** (ADR-0087).

### Added
- **Drop a stash directly.** A new Drop action deletes a stash entry without
  touching the working tree, behind a danger-confirm modal that shows how to
  recover it (`git stash store <oid>`). The dropped commit is recorded in the
  operation log (ADR-0087).

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
