# ADR-0137: Graph | PRs | Editor are three modes, not two toggles

- Status: Accepted
- Date: 2026-08-19

## Context

Editor mode (T-WS-EDITOR-001) and PR mode each shipped as a *toggle* whose
button showed what it would switch **to**: the Editor button read "Editor" with
a pencil in graph mode and "Graph" with a waypoints glyph while the editor was
open, and PR mode copied the pattern.

That reads fine with one takeover. With two it breaks: `resolve_workspace`
ranks PrMode above Editor, so both can be open at once with only PR mode
visible — and then **both buttons read "Graph"**, neither saying where you
would actually land. Pressing Editor from PR mode appeared to do nothing (the
toggle closed the invisible editor); pressing PRs' "Graph" revealed the editor
instead of the graph. Both were reported as bugs.

## Decision

Graph, PRs and Editor are three mutually exclusive **modes** with one button
each. A button always names its own mode and is highlighted while that mode is
on screen. There is no toggle-off: Graph is how you leave a takeover.

| Button | Effect |
|---|---|
| Graph | closes PR mode, File History and Ecosystem; closes the editor via the existing dirty guard |
| PRs | shows PR mode; an open editor stays alive underneath (unsaved buffers survive) |
| Editor | reveals or creates the editor; PR mode steps aside |

`KagiApp::workspace_mode()` derives the current mode from the same precedence
`resolve_workspace` uses, so the toolbar can never disagree with the screen.

The highlight is a wash in `theme().color_branch` — each theme's primary colour
(apple-dark yellow, catppuccin blue, dracula purple) and already the toolbar's
count-chip colour. Not `theme().accent`, which is the cherry-pick mauve and is
purple in nearly every theme regardless of the theme's own palette.

`view.showGraph` (`Cmd-Shift-G`) joins the existing `view.togglePrMode`
(`Cmd-Shift-P`) and `view.toggleEditorWorkspace` (`Cmd-Shift-E`) as a normal
command, so it inherits the View menu entry and user keybinding overrides.

## Consequences

- "Both open" remains representable in state (two independent `Option`s); only
  the UI is exclusive. Keeping the editor alive behind PR mode is what makes
  PRs → Editor lossless.
- Leaving PR mode for Graph does not preserve an editor, by design: Graph means
  graph. The dirty guard still runs, so nothing is discarded silently.
- Adding a fourth mode means a fourth button and a `workspace_mode()` arm — no
  new toggle semantics to reason about.
