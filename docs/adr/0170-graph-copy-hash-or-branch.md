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
- **Wiring**: reuse the existing `CopyDiffSelection` handler. When there is no
  diff-row selection, the Graph gets first refusal — if the root (Graph) holds
  focus and a row is selected, copy that row's value; otherwise `cx.propagate()`
  as before so the TextView selection is copied. The `!Terminal && !Input`
  keybinding predicate + the diff-selection guard keep it off text selections.
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
