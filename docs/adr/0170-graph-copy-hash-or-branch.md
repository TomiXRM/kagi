# ADR-0170: Graph Cmd+C copies the selected row's hash or branch

- Status: Accepted
- Date: 2026-09-04
- Touches: `crates/kagi-ui-core/src/settings.rs`,
  `crates/kagi-ui-core/src/i18n/mod.rs`,
  `src/ui/commit_list.rs`, `src/ui/mod.rs`, `src/ui/render.rs`,
  `src/ui/settings_view.rs`

## Context

When a commit row is selected in the Graph, there was no keyboard way to copy
its identity — the user had to right-click and use the context menu. Cmd+C
(macOS) / Ctrl+C (elsewhere) is the expected shortcut, but the key is already
bound (`secondary-c` → `CopyDiffSelection`, guarded `!Terminal && !Input`) to
copy the diff line selection / TextView window selection. We must add the Graph
copy without hijacking Cmd+C when the user has a real text selection.

## Decision

A setting `graph_copy_target` (flat string, `"hash"` | `"branch"`, default
`"hash"`) selects what Cmd+C copies from the selected Graph row. Hash is the
default because every commit has a hash but not every row carries a branch.

- **Resolver** (pure, unit-tested): `commit_list::graph_copy_value(badges,
  full_sha, target) -> String`. `Hash` → the full 40-char SHA. `Branch` → the
  first *local* branch ref on the row (HEAD branch counts; local preferred over
  remote/tag); with no local branch it falls back to the full SHA, so the copy
  never yields nothing.
- **Wiring**: focused diff bodies first handle their own valid owner/content
  revision selection (ADR-0124, #1444), never another surface's saved range.
  The root `CopyDiffSelection` handler gives the Graph first refusal only when
  `commit_list_has_focus` permits commit navigation: root focus, no front layer,
  no Conflict/Home takeover, and the resolved center is CommitList. A selected
  row then copies its value; otherwise `cx.propagate()` lets the foreground
  TextView selection copy. The `!Terminal && !Input` keybinding predicate remains.
- **Feedback**: a `Copied <value>` toast (reuses `i18n::copied_fmt`, EN+JA) and
  a contract log line — `graph: copied hash <sha>` or `graph: copied branch
  <name>` via `klog!`.
- **Settings UI**: a two-way segmented choice (RadioGroup) in the Appearance
  section writes the flat key.

## Consequences

- No git2 in the UI (the resolver is pure over already-built row badges).
- The full SHA is copied (not shortened) — standard for copy; the user can
  shorten. The branch preference intentionally ignores remotes/tags.
- Because the resolver reads `Settings::graph_copy_target()` per copy, changing
  the setting takes effect immediately with no reload.

## Amendment — 2026-10-13: pointer focus and foreground Copy (#1447, #1448)

Root focus is a keyboard owner, not proof that Graph is visible. Branch Cleanup,
PRs, Issues, File History, Home and modals can retain a selected Graph row while
replacing or covering its center. Copy therefore reuses the same production
visibility/front-layer predicate as commit navigation instead of checking root
focus alone. Hash/branch resolution, settings, feedback and contract logs do not
change; an ineligible Graph propagates Copy to the existing text owner.

The commit row's left-click selection and right-click context opener explicitly
focus the existing root handle before selecting/opening. The menu's retained
return-focus target is consequently Graph, so Escape returns there rather than
to the previous filter/sidebar/control. Rendering, lazy reads and typing do not
move focus. No additional focus owner, selection model or Git write is introduced.

Tier A `graph_pointer_focus` covers real pointer handoff, field isolation,
subsequent arrows and hash/branch copy, sidebar/toolbar departure, context Escape
and selection geometry across EN/JA, Apple Light/Dark, both requested viewports
and 100%/167%. `graph_copy_foreground` checks retained hidden selection, clipboard
sentinel preservation across takeovers/front layers, foreground TextView copy,
Graph return, focused diff Copy and retirement after closing its pane. Both
assert unchanged Git state.
