# Building modern UI in Kagi

How to build or change a Kagi screen so it looks and behaves like a current
desktop tool, not a control that merely passes its test. Read this before any
UI work. It is the synthesis of three documents (#931):

- [research-gpui-component.md](research-gpui-component.md) — what the pinned
  gpui-component 0.5.2 (`b004e595`) and Kagi's UI actually are (facts).
- the PM's first draft (a global token table; superseded) and
- [critique-of-pm-draft.md](critique-of-pm-draft.md) — why a global table
  breaks Kagi.

## What went wrong, and what "modern" means here

"Modern" is not a library and not decoration. Zeron looks modern because it
keeps **role colours, sizes, spacing, states and motion consistent across
screens**, not because it imports particular components (research §Zeron).
Kagi's dated spots come from the same gap, seen from the other side:

- Kagi already uses gpui-component where it matters (Input 21×, Button 66×,
  Switch/Select/Checkbox/Tooltip; no hand-made text input exists). Swapping
  `div()` for components does not by itself make anything modern — and the
  pinned Tab, Sidebar and Toggle have no keyboard focus, PopupMenu has no
  focus ring, so "uses the library" does not mean "accessible" either.
- What is missing is a **per-role target** (size, radius, spacing, every
  state) and a **comparison against a reference** before merge. Issues
  describe behaviour; Tier B screenshots prove the screen renders, not that it
  matches what a good version looks like.
- The cause of a specific dated control is *not known until measured*. Kagi
  already maps Input colours into gpui-component's theme; the user's "text
  boxes look old" could be size, radius, font, padding, focus, placeholder or
  background. Find out with a before/reference/target comparison (below)
  before changing a shared default.

A control is modern when, for its role:

1. **Every state is designed** — rest, hover, pressed, keyboard
   focus-visible, disabled (with the reason reachable), selected, loading,
   error, empty — and checked in the real app.
2. **It shares its role's geometry and colour roles** with its siblings
   instead of one-off `px(…)` values.
3. **Hierarchy is clear** — one primary action, quieter secondaries, danger
   distinct; text in two or three weights and colours.
4. **The non-happy paths exist** — empty, loading, offline/refused, too many,
   long text — each with a sentence and, where useful, one action.
5. **It works from the keyboard and for assistive tech** — reachable, focus
   visible, role/name/state exposed — and keeps Kagi's safety contracts.

## The procedure (every UI change)

1. **Name the role and the density.** Which role from the table below? Is the
   surface dense (sidebar, graph, file lists, modal targets) or open (Home,
   Settings, empty states)? Density is decided per surface, never loosened
   globally.
2. **Pick a reference.** One concrete screen from a product (GitHub, Linear,
   Raycast, ChatGPT, macOS, Zeron…) for the same role, with what to take and
   what *not* to take. Put it in the issue.
3. **Look for the part before building it.** In this order: an existing Kagi
   helper (`KagiButton`, `modal_shell`, `list_a11y`, `menu_overlay`,
   `sidebar_*`, `button_style`); a gpui-component part in the pinned
   source and its story (`crates/story`, run it — research §pinned story);
   only then a hand-made control. A hand-made interactive control states in
   the PR why the part did not fit (focus, safety, virtualisation, density).
4. **Write the state contract** before code: keyboard path and focus-visible,
   AX role/name/state, disabled reason, loading/empty/error, and any Kagi
   safety contract the control touches (plan → confirm, one modal slot,
   oplog-owned errors, Esc/Enter routing).
5. **Build to the role target**, using the theme roles and `scaled_px` /
   rem so zoom (0.7–1.5) scales it.
6. **Compare, then merge.** Tier B: screenshot *before*, *reference* and
   *after* of the same screen under the matrix that applies (light + dark,
   EN + JA, zoom 1.0 and 1.25 or 1.5, a narrow width, long text, many rows).
   In the PR, list what changed and why the remaining differences from the
   reference are deliberate. Tier A asserts the state contract and geometry
   (bounds inside the window, `header_fit`-style).

## Role targets (start here; change only with a measured reason)

Sizes are the pinned gpui-component scale (16px/rem, scaled with zoom) unless
the row says a Kagi geometry is kept on purpose.

| Role | Target | Keep / exceptions |
|---|---|---|
| Text input (forms, settings, composer) | gpui-component `Input` M = 32 high, radius 6 (`Theme.radius`), 1px border that takes the ring colour on focus | Small (24) in toolbars and filter strips. Placeholder must render — if it does not (#930 found the overlay workaround), fix it once in a shared wrapper, not per screen. |
| Search — inline filter | `Input` S = 24, leading search icon, clear (×) when non-empty | Command palette keeps Cmd/Ctrl+P, subsequence fuzzy match and single-Esc close. |
| Search — hero (Home) | larger box, leading icon, no heavy border, results grouped by heading | Only where search is the screen's main action. |
| Button | gpui-component `Button` / `KagiButton`: S = 24 in toolbars, M = 32 in forms and dialogs, radius 6 | Toolbar buttons that are unavailable stay clickable to show the reason (render_header); do not convert them to `.disabled(true)`. |
| List row (dense) | keep: sidebar 20, worktree 24, graph commit row 29 (synced with the lane canvas), modal target 18 | Hover is a full-width wash; selection keeps Kagi's colour meanings (branch/ref/status). Changing a row height needs the visible-row count, virtual list height and a11y positions shown together. |
| List row (open: Home, PR/Issue lists) | Height follows the actual content and surface, not one open-row size: the PR dashboard row settles near 65px around a 40px avatar; the Issue row has a 104px minimum for its title and metadata ([`pr_dashboard.rs`](../../src/ui/pr_dashboard.rs#L341-L344), [`issues_mode.rs`](../../src/ui/issues_mode.rs#L486-L515)). | 32–40px is only a candidate for a single-line result without those contents, not a PR/Issue list target. Keep the identifying part of long names; the full value must be reachable by keyboard/AX, not only a tooltip. |
| Menu / context menu | Kagi `menu_overlay` (keeps disabled-reason tooltips and danger rows); pinned PopupMenu rows are 26 fixed | Adopt PopupMenu only where those contracts are not needed. |
| Modal / confirmation | Kagi `modal_shell` (target list not hideable behind disclosure; long lists scroll inside their panel; fixed action row; existing widths 504/576/648) | The target rows are not all simultaneously on-screen when the list is long ([`modal_shell.rs`](../../src/ui/modal_shell.rs#L440-L461)). Do not replace with gpui-component Dialog (448 fixed, different focus/Esc). Borrow its look, not its behaviour. |
| Toast | Kagi `toast_stack` (bounded preview, max 4, info 4 s / error 8 s); the Operation Log owns detail | Not gpui-component Notification. |
| Tabs / segmented | Kagi's workspace-mode nav exposes `Role::Tab`/`TabList`; its selected label uses `accent_text_on(surface)` (at least 4.5:1). | The repo tab strip has no `Role::Tab` or focus/key handling, and workspace-mode cells also lack focus/key handling ([`tabs.rs`](../../src/ui/tabs.rs#L885-L919), [`workspace_mode.rs`](../../src/ui/workspace_mode.rs#L40-L60)). Do not claim keyboard-complete tabs until each surface is implemented and checked. Pinned Tab/TabBar has no keyboard focus either. |
| Toggle / checkbox / radio | gpui-component `Checkbox` for selection, `RadioGroup` for a mutually exclusive choice; `Switch` is already used for immediately applied *app preferences*. | Pinned `Switch` handles mouse down but provides no focus/key path or AX role/name/checked state ([`switch.rs`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/ui/src/switch.rs#L142-L225)). Add those behaviours or use an accessible alternative before calling the control keyboard/AX-complete. A toggle that changes the repository still needs the Git write pipeline. |
| Empty / loading / error | a sentence + one action; skeleton rows that match the final layout for lists; error with what to do and a retry | Git write errors: short toast + Operation Log, never a permanent toast. |
| Motion | instant or ≤ 150 ms for hover/press; Kagi-owned animations honour `reduce_motion` | A library transition that cannot be turned off is listed as an exception in the PR. |

## In an issue for UI work

Add three lines instead of a style table: **role(s)**, **reference** (link or
screenshot, what to take / not take), **states that must exist** (from the
contract above). Acceptance includes the before/reference/after comparison.

## Open questions (to settle with evidence)

- What actually makes the text boxes look dated — measure on one screen
  (#931 pilot) before changing `Theme.radius`, font size or input padding
  globally.
- Whether to own a small `kagi-ui-core` input/button wrapper that fixes the
  placeholder and focus treatment once.
- Which hand-made controls (research: 176 builders) are worth migrating —
  each needs its own before/after and contract, not a sweep.
