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
6. **The control explains itself; the screen does not explain the control.**
   No instructional prose in the UI ("To delete this worktree, right-click
   and choose…"). The affordance — a button, a menu item, an icon, a short
   state label — is the explanation. Apple's apps and current web apps do not
   ship how-to paragraphs next to controls; neither does Kagi. A needed
   warning is a short state ("Uncommitted changes"), not a procedure.
   Icons carry type and state (folder, branch, lock, terminal, warning) so a
   row is not a line of text.

What this guide changes was measured once (#931 A/B: the same "Go to ref"
popover built by two agents from one base, one with this file and one
without). Where the screen has sibling parts to copy — there, the command
palette's frame, `Input`, rows and `KagiButton` — both results looked the
same: an agent reuses siblings without being told. The difference was in
verification and record: only the run with the guide wrote the PR block,
recorded what it left out and why, and listed what it had not observed
(real-window focus, IME, AX); the other never launched the GUI. So expect
this file to change *how a change is checked and reported*, not how a
screen with siblings looks. Its effect on looks is to be measured on a
screen with no siblings to copy (#934's worktree row).

## When the procedure applies

| Change | What is required |
|---|---|
| **Visual or structural** — a new screen, control or layout; a change to size, spacing, colour role or hierarchy | The whole procedure below, and the PR block. |
| **State of an existing part** — a new disabled/loading/error state, a keyboard path, an AX name | Steps 3–6 for that part only; the reference may be omitted. |
| **Wording only**, or no visible change | Nothing from this file. |

## The procedure

1. **Name the role and the density.** Which role from the table below? Is the
   surface dense (sidebar, graph, file lists, modal targets) or open (Home,
   Settings, empty states)? Density is decided per surface, never loosened
   globally.
2. **Pick one reference state.** One screen of one product (GitHub, Linear,
   Raycast, ChatGPT, macOS, Zeron…) for the same role, with what to take and
   what *not* to take. The reference is that product's screen as it is; it is
   not re-rendered under Kagi's themes or zoom.
3. **Look for the part before building it**, in this order:
   1. an existing Kagi helper and its call sites (`KagiButton`,
      `button_style`, `modal_shell`, `list_a11y`, `menu_overlay`,
      `sidebar_*`, `toast_stack`);
   2. the pinned gpui-component source (`Cargo.lock` rev, `crates/ui/src`);
   3. if that part has a story, run it from a checkout of the same rev
      (`cargo +stable build --locked -p gpui-component-story`, see
      [research §pinned story](research-gpui-component.md#pinned-story-の実画面native-macos)) —
      it shows the upstream Default Light look, not Kagi's;
   4. then look at the result inside Kagi.
   A hand-made interactive control says in the PR why the part did not fit
   (focus, safety, virtualisation, density).
4. **Write the state contract** before code: for each state, *required* or
   *not applicable* with a reason. States: rest, hover, pressed,
   focus-visible, disabled (and how its reason reaches mouse, keyboard and
   AX), selected, loading, empty, error; plus every Kagi safety contract the
   control touches (plan → confirm, one modal slot, oplog-owned errors,
   Esc/Enter routing, IME).
5. **Build to the role target**, using theme roles and `scaled_px` / rem so
   zoom (0.7–1.5) scales it. State which owner a value belongs to:
   `sync_gpui_component_theme` (shared — `Theme.radius` changes every
   component that reads it), a Kagi token, or a component's fixed value.
6. **Verify, then compare.**
   - **Tier A** (scoped with `KAGI_GUI_E2E_ONLY`): deterministic states and
     geometry — which state shows, bounds inside the window
     (`header_fit`-style), row counts. Tier A does not prove focus or IME
     (TestDispatcher, see the verify skill).
   - **Tier B**: the real app. Pointer, keyboard path, IME Enter/Esc, and the
     AX tree (Accessibility Inspector) where the contract asks for them.
     Screenshots of **before** and **after** with the same data, viewport,
     theme, language, zoom and state; the reference sits beside them.
   - **Choose the matrix by risk**, not all combinations: zoom 1.0 plus the
     end the change can break (0.7 for small text, 1.5 for clipping);
     light and dark when a colour or token changed (and a custom theme when a
     theme role changed); EN and JA with long text when text width matters;
     the narrow widths that apply (600/700/750/900); many rows for lists.
   - **Record what was not observed** and why. An unobserved item is not
     "done".

## The PR block

```
UI: <surface> — role <role>, density <dense|open>
Reference: <product screen + link/image>; take <…>; not take <…>
Parts: <helper/component used>; rejected <part> because <reason>
States: rest ✓ hover ✓ focus-visible ✓ disabled ✓ (reason via …) loading n/a (…) …
Safety contracts kept: <…>
Values: <value> owned by <theme bridge | Kagi token | component fixed>
Verified: Tier A <scenarios>; Tier B <matrix chosen + why>; not observed: <…>
Before / after: <images>
```

## Role targets (start here; change only with a measured reason)

Sizes are the pinned gpui-component scale (16px/rem, scaled with zoom) unless
the row says a Kagi geometry is kept on purpose.

| Role | Target | Keep / exceptions |
|---|---|---|
| Text input (forms, settings, composer) | gpui-component `Input` M = 32 high, radius 6 (`Theme.radius`), 1px border that takes the ring colour on focus | Small (24) in toolbars and filter strips. The placeholder must render. #930 (unmerged) found it missing in one place and overlaid a hint; find the cause and its conditions (same InputState, IME, zoom) before adding any shared wrapper that could change existing filters. |
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
| Pane open / close | Kagi `panel_motion` ([`panel_motion.rs`](../../src/ui/panel_motion.rs)), shared by the bottom panel, the left sidebar and the right pane: open 180 ms ease-out, close 150 ms ease-in; a toggle in mid-flight turns around from where the pane is, its time scaled by the distance left. Only the outer box's size moves; the content keeps its size and is clipped (`panel_motion::clip`), so the Terminal's grid and the panes' layouts never change while they slide. While a pane slides, drags of its divider are ignored (the sidebar's, with the Graph's badge and graph-column dividers that follow its edge; the right pane's; the bottom panel's — [`render_divider.rs`](../../src/ui/render_divider.rs), #957): the divider is drawn at the moving edge, and a drag turns the cursor into the saved size. | A pane appears or disappears at once when the layout, not its own toggle, changed it (a takeover, Conflict, Editor), and with `reduce_motion`. Use it for any new pane that opens and closes rather than a second timing. |

## In an issue for UI work

Add three lines instead of a style table: **role(s)**, **reference state**
(link or screenshot, what to take / not take), **states that must exist**
(from the contract). Acceptance includes the before/after comparison and the
PR block.

## Known gaps (do not claim these are met)

- Settings' `Switch` has no keyboard path and no AX role/name/checked state.
- The repo tab strip has no `Role::Tab` and no keyboard handling; the
  workspace-mode cells have the role but no keyboard handling.
- Toolbar buttons expose AX disabled, but their reason reaches only a mouse
  click (footer); keyboard/AX users do not get it.
- Library transitions (Switch 150 ms, Tab 200 ms, Dialog 250 ms) do not follow
  `reduce_motion`.

## Open questions (to settle with evidence)

- What actually makes the text boxes look dated — measure on one screen
  (#931 pilot) before changing `Theme.radius`, font size or input padding
  globally.
- Whether to own a small `kagi-ui-core` input/button wrapper that fixes the
  placeholder and focus treatment once.
- Which hand-made controls (research: 176 builders) are worth migrating —
  each needs its own before/after and contract, not a sweep.
