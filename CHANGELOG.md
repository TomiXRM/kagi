# Changelog

All notable changes to Kagi are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

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
