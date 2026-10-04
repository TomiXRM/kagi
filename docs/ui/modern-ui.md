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
  pinned Tab, Sidebar and Toggle have no keyboard focus (Kagi wraps them:
  `keyboard_nav`), PopupMenu has no focus ring, so "uses the library" does
  not mean "accessible" either.
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
| Button | gpui-component `Button` / `KagiButton`: S = 24 in toolbars, M = 32 in forms and dialogs, radius 6 | The vertical `render_header` toolbar is a Kagi geometry exception: each button (including unavailable) is one Tab stop with a `keyboard_nav::with_ring` focus-visible border. An unavailable action keeps its click handler (never `.disabled(true)`), exposes its existing footer reason via `aria_description` alongside the visible AX name and AccessKit disabled state, and Enter / Space show that same reason in the footer. |
| List row (dense) | keep: sidebar 20, worktree 24, graph commit row 29 (synced with the lane canvas), modal target 18 | Graph author/time are `text_xs`: author is proportional and truncated in a 96px zoom-scaled column (full name in tooltip/AX); time is monospaced, right-aligned in a 48px zoom-scaled column with compact ages (`36m`, `5h`, `3d`, `2mo`), while AX keeps the full relative age. Hover is a full-width wash; selection keeps Kagi's colour meanings (branch/ref/status). Changing a row height needs the visible-row count, virtual list height and a11y positions shown together. |
| List row (open: Home, PR/Issue lists) | Height follows the actual content and surface, not one open-row size: the PR dashboard row settles near 65px around a 40px avatar; the Issue row has a 104px minimum for its title and metadata ([`pr_dashboard.rs`](../../src/ui/pr_dashboard.rs#L341-L344), [`issues_mode.rs`](../../src/ui/issues_mode.rs#L486-L515)). Home's rows: `keyboard_nav::RowFocus` / `RowList` — the list is one Tab stop (the row last focused while it is drawn, else the first row on screen), ↑/↓ and Home / End / PageUp / PageDown scroll the destination row into view and focus it (Cmd+↑/↓ also move to the ends), Enter/Space press it; a focused row that leaves the list hands the focus to the first row left, or to the window — also when the list itself is not drawn (still loading, failed: `RowFocus::release`) (#959, #986; in #960 every row was a Tab stop). | 32–40px is only a candidate for a single-line result without those contents, not a PR/Issue list target. Keep the identifying part of long names; the full value must be reachable by keyboard/AX, not only a tooltip. The Graph sidebar also has a per-pane row keyboard path (#987); the PR / Issue navigators still do not. |
| Menu / context menu | Kagi `menu_overlay` (keeps disabled-reason tooltips and danger rows); pinned PopupMenu rows are 26 fixed. The context menus' keyboard is `menu_keys` (#985): an opened menu takes the focus on its first enabled item, ↑/↓ (wrapping) and Home/End move among the enabled items, Enter/Space press one, Escape, Tab and Shift+Tab close; on close the focus goes back where it was (to the window when an item opened a modal). A menu taller than the window scrolls its items and keeps the keyed item in view; leaving the tab, a landed reload, or a modal / notice / plan drawn over the menus closes them. `Role::Menu` / `Role::MenuItem`; a disabled item carries the AX disabled state and its reason as `aria_description`; the focus shows as the hover highlight, only for `focus_visible`. Shift+F10 (and the Menu key) opens the selected commit's menu below its row from the window, and a focused sidebar row's menu below that row. | Adopt PopupMenu only where those contracts are not needed. A new context menu passes `Some(&app.menu_keys)` to `render_menu_overlay` and is added to `KagiApp::any_context_menu_open`. |
| Modal / confirmation | Kagi `modal_shell` (target list not hideable behind disclosure; long lists scroll inside their panel; fixed action row; existing widths 504/576/648). Six input + confirm cards (#956: Create Branch/Tag, Stash, Add Worktree, Rename Branch, Set Upstream) use a heading with inline operation icon, form `Input`/confirm M = 32 as the base, errors immediately below the field, a visible disabled confirm when invalid/blocked, and only a Git command for recovery when the plan is Ready. | The target rows are not all simultaneously on-screen when the list is long ([`modal_shell.rs`](../../src/ui/modal_shell.rs#L440-L461)). Do not replace with gpui-component Dialog (448 fixed, different focus/Esc). Stash keeps its 38px message field and 24px action buttons; Create Branch has no recovery row, Set Upstream no recovery command. Borrow Dialog's look, not its behaviour. |
| Toast | Kagi `toast_stack` (bounded preview, max 4, info 4 s / error 8 s); the Operation Log owns detail | Not gpui-component Notification. |
| Tabs / segmented | Kagi's `keyboard_nav::TabList` ([`keyboard_nav.rs`](../../src/ui/keyboard_nav.rs)): `Role::Tab`/`TabList`, one Tab stop (the cell the arrows moved to while the focus is in the list, else the selected cell — Tab / Shift+Tab leave in one press, coming back in lands on the selected cell), ←/→/Home/End between cells, Enter/Space select, a `focus_visible` ring in the Input ring colour, focus handed back after a pointer click. Automatic activation where selecting starts nothing (Home's switch); manual where it starts a read (the workspace-mode nav, the repository tab strip — switching a tab reads it again). A control inside or beside the cells (the strip's × and +) is not a Tab stop of its own; its keyboard path is a command (⌘W closes the tab in front, ⌘T opens Home). Cells are keyed by identity (a tab's `TabId`, Home's own key), so a tab added, closed or moved keeps its focus; a closed tab whose cell held the focus hands it to the window (`TabFocus::release_closed`). The selected label uses `accent_text_on(surface)` (at least 4.5:1). The content the list switches goes through `tab_panel_a11y::tab_panel` ([`tab_panel_a11y.rs`](../../src/ui/tab_panel_a11y.rs), #979, #983): `Role::TabPanel`, named after the selected tab without its count, on an element that is already there (no wrapper, so layout and key context stay): Home's body under the switch; the sidebar page under the workspace-mode nav, only while Graph / PRs / Issues is selected; for the repository tab strip, the workspace body row (sidebar, center with the bottom panel, right pane — the toolbar, operation strip and status bar stay outside) named after the tab, and Home's body named Home. | Pinned Tab/TabBar has no keyboard focus. In Conflict Mode the conflict view replaces the body row and is no panel (#983). Pinned gpui has no `aria-labelledby` / `aria-controls`, so a panel repeats its tab's name as its own label; no panel is tied to its tab by a relation. |
| Keyboard focus on a Kagi-drawn control | `keyboard_nav` ([`keyboard_nav.rs`](../../src/ui/keyboard_nav.rs)): a roving tabindex — a tab list (`TabList`) or a virtualized list's rows (`RowFocus` / `RowList`) is one Tab stop, the arrows move inside it; the ring is a fixed 2px border (`RING`, not scaled with zoom so it stays crisp at 0.7×) in the theme's `color_branch`, drawn only for `focus_visible`; a control that carries it pads with `keyboard_nav::inset(n)` (the zoomed padding minus the ring), so its outer size is what `scaled_px(n)` gave before at every zoom. | Do not add a second ring or focus helper; a new list or tab row goes through these. A control inside a row or cell (Open, Clone, a tab's ×) is a Tab stop of its own only where it has no other keyboard path. |
| Toggle / checkbox / radio | gpui-component `Checkbox` for selection, `RadioGroup` for a mutually exclusive choice; `Switch` for immediately applied *app preferences*, always through Kagi's `keyboard_nav::switch` ([`keyboard_nav.rs`](../../src/ui/keyboard_nav.rs), #970): the Switch draws it and keeps its pointer handling; around it one Tab stop per toggle with `Role::Switch`, the row's title as its name and `aria_toggled` checked / unchecked, Enter and Space flip it (gpui's keyboard click, the key-down stopped), and the `focus_visible` ring sits in a negative margin, so the control keeps its size. | Pinned `Switch` alone has no focus/key path or AX role/name/checked state ([`switch.rs`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/ui/src/switch.rs#L142-L225)): never use it bare. A toggle that changes the repository still needs the Git write pipeline. |
| Empty / loading / error | a sentence + one action; skeleton rows that match the final layout for lists; error with what to do and a retry | Git write errors: short toast + Operation Log, never a permanent toast. |
| Motion | instant or ≤ 150 ms for hover/press; Kagi-owned animations honour `reduce_motion` | A library transition that cannot be turned off is listed as an exception in the PR. |
| Pane open / close | Kagi `panel_motion` ([`panel_motion.rs`](../../src/ui/panel_motion.rs)), shared by the bottom panel, the left sidebar and the right pane: open 180 ms ease-out, close 150 ms ease-in; a toggle in mid-flight turns around from where the pane is, its time scaled by the distance left. Only the outer box's size moves; the content keeps its size and is clipped (`panel_motion::clip`), so the Terminal's grid and the panes' layouts never change while they slide. While a pane slides, drags of its divider are ignored (the sidebar's, with the Graph's badge and graph-column dividers that follow its edge; the right pane's; the bottom panel's — [`render_divider.rs`](../../src/ui/render_divider.rs), #957): the divider is drawn at the moving edge, and a drag turns the cursor into the saved size. | A pane appears or disappears at once when the layout, not its own toggle, changed it (a takeover, Conflict, Editor), and with `reduce_motion`. Use it for any new pane that opens and closes rather than a second timing. |

## In an issue for UI work

Add three lines instead of a style table: **role(s)**, **reference state**
(link or screenshot, what to take / not take), **states that must exist**
(from the contract). Acceptance includes the before/after comparison and the
PR block.

## Known gaps (do not claim these are met)

- The PR / Issue navigators still lack a row keyboard path.
- The Graph sidebar's secondary controls remain pointer-only: a pane cannot
  be collapsed from the keyboard, the worktree card's Refresh and port link
  are pointer-only, and branch-row deletion has no keyboard path (its
  context menu has one, #985). Closing a checkout plan opened from a row
  returns focus to the window, not to the row.
- The conflict file menu, the Editor tree menu and the list filter menu
  share `menu_overlay` but not its keyboard (`keys: None`): Escape does not
  close the first two through the window's cancel chain (#985).
- GPUI shows `focus_visible` only while the last input was a key: `MouseMove`
  hides the ring without moving focus, but Enter / Space still activates the
  focused control. A GPUI/Kagi fix is deferred (#354 gap 1).
- A tab's `Role::TabPanel` has no `aria-labelledby` / `aria-controls` relation
  back to its tab; the panel repeats the tab name instead (#983).
- Actual VoiceOver readout of unavailable toolbar reasons is not verified.
  Keyboard access and `aria_description` are wired (#975), but a direct
  `AXUIElement` probe does not activate GPUI's accessibility tree (#354).
- Library transitions (Switch 150 ms, Tab 200 ms, Dialog 250 ms) do not follow
  `reduce_motion`.
- Settings' gpui-component Root focus trap searches at most 100 focus moves
  when wrapping Tab / Shift+Tab. If the window has more Tab stops, focus
  may still leave Settings; the original Terminal Tab escape is fixed, but
  this search bound remains (see `docs/decisions.md`, #974).
- Linux / FreeBSD platform-menu overlays still need keyboard and focus
  verification when combined with Settings or a confirmation modal: the
  three-layer Escape order, command-overlay dismissal, and tab / repository
  switches with a dropdown open remain unverified (#990). The resolved
  `front_layer` / `Z_ORDER` priority and modal veto are not this gap (#976).
- UI-thread repository work still blocks interaction for stage / unstage /
  hunk writes, which run synchronously under a lease until the write queue
  lands (#355 stage 3). Snapshot creation and conflict continue / skip run in
  background guard writers, and the post-stage WIP diffstat is an ordered
  background scan (#996).

## Resolved: dated text boxes in input + confirm cards (#956)

The Create Branch pilot found a 40px circular badge and long title that
made the header top-heavy; explanatory text crowded a 24px form `Input`.
Errors sat away from the field, and invalid/blocked plans hid confirm.
Six cards now use shorter titles with inline icons and base M = 32 form
`Input`/confirm, with errors immediately below the field. Invalid or
blocked plans keep a visible disabled confirm; recovery, when present, is
only a Git command for Ready plans (detail remains in Operation Log).
Shared `Theme.radius`/font/input padding and `modal_shell`/plan/IME stay unchanged; values align by role.

## Plan comparison and Operation Log restore (#988)

The shared plan card keeps CURRENT and PREDICTED as equal-width columns with a
centered arrow; branch and status chips stay on one scrollable line per column.
The operation plan remains the source of truth. Show an equivalent CLI command
only when it faithfully describes the executable plan. Pull has none: its
execution fetches again and may merge a newly diverged remote even if the
cached tracking ref indicated a fast-forward at planning time. Where a Ready
plan with no blockers does have an equivalent, the collapsed command is a
keyboard/screen-reader accessible button (Enter/Space) with its own Copy
button; Copy all includes its full text. Blocked and no-op plans omit that
command from both the card and Copy all, even if the backend supplied one.

Operation Log restore has its own REFS-first card, not a second generic plan
summary. It uses the same command-visibility gate as the shared plan card:
Ready with no blockers. Show each
planned ref's expected and destination OIDs, including red-tinted deletions.
Long ref-name chips cap at 45% of their row with ellipsis; their tooltip and
accessibility name retain the complete canonical ref, while the OIDs remain
visible. Then show unchanged worktree/index/untracked/stash/remotes and
warnings. The first three ref targets stay visible at compact window sizes;
the refs list scrolls independently when longer. Draw the AFTER graph from the
loaded tab's existing commit rails: moved branch badges go to their
destination, commits no longer reached by any ref are muted, and retained
commits stay normal. The preview fits its rows up to six, then scrolls
independently; lane pitch, rail width and horizontal follow-scroll use the same
scaled pixels so a focused far-right lane stays visible at zoom > 100%. Do not
claim a graph projection for an unloaded destination or
a moved annotated tag. Ref rows are decoded once at admission; malformed
rows fail closed with a durable Failed receipt and a bounded UI preview.
Restore keeps its two confirmations and backend preflight/verification/
recording unchanged.

## Open questions (to settle with evidence)

- Whether to own a small `kagi-ui-core` input/button wrapper that fixes the
  placeholder and focus treatment once.
- Which hand-made controls (research: 176 builders) are worth migrating —
  each needs its own before/after and contract, not a sweep.
