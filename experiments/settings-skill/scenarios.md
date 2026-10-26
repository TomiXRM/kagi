# Fixed scenarios and requirement checklists

Frozen before iteration 1. Scoring: ○ = 1, partial = 0.5, × = 0; accuracy =
sum / items. A run succeeds only when every `[critical]` item is ○. Items are
never re-tagged or reworded after iteration 1.

Common definitions used by the items:

- **Host render**: `cargo run --example <file>` with the gallery host
  environment (`KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`,
  `KAGI_GALLERY_OUT`) writes a PNG of a real native window and exits 0; an
  unknown state name exits non-zero and lists the valid states.
- **Kit reuse**: every section, group, row, switch, segmented control, pop-up
  value, stepper, navigation row, icon tile, sidebar item and capsule button on
  the page comes from `kagi::ui::settings_kit`. A page-local component is
  allowed only for a control the kit does not have, and the report names it as
  a kit gap.
- **Kit geometry**: row heights, hairlines, group radius and trailing control
  alignment are the kit's (no page-local `px(...)` for these).

## S1 — Rex "Notifications" (median)

Rebuild Rex's Notifications pane (`rex-notifications.png`) inside the Kagi
settings scaffold: sidebar with Rex's categories (Notifications selected), page
title "Notifications", and in the detail pane:

1. Section "In-App Notifications" with one row "Notification position /
   Displayed bottom right" whose trailing control is a 3×2 position picker
   (selected cell bottom-right shows a check).
2. Section "Program Activity" with subtitle "How status from processes and
   agents is handled": a group of four switch rows (Show activity in tabs,
   Show system notifications, Badge Dock icon, Play sounds — all on).
3. A group of four navigation rows with a leading status symbol, title and
   subtitle, and a trailing chevron: Working / Tabs; Blocked / Tabs,
   Notifications, Dock Badge, Sound; Error / same; Done / same.
4. A group of two switch rows: Include idle processes in activity (on), Show
   each process in tabs (off).

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. Structure matches the reference order and content above, including the
   subtitle line and the off state of the last switch.
4. The position picker is a reusable function with explicit parameters
   (selected cell, forced state), coloured only through theme tokens, and its
   named states (at least rest, selected, focus) appear in the page's state
   list.
5. Kit geometry holds in the capture: trailing controls share one edge;
   rows with and without subtitles use the kit heights.
6. The page exposes named states (at least `default` plus two forced
   interaction states) that render visibly different captures.
7. The report lists the commands run, the PNG paths and what was not observed.

## S2 — Rex "Interface" (edge: new visual controls)

Rebuild Rex's Interface pane (`rex-interface.png`): sidebar (Interface
selected), page title, and:

1. Section "Appearance": one group containing two labelled choice clusters
   separated by a vertical divider — "Color scheme" (Auto, Light, Dark; Auto
   selected) and "Interface style" (Glass, Classic; Classic selected) — each a
   preview tile with a label underneath; then a row "Tint with theme" (on).
2. Section "Tabs": Tab style (segmented Above / Left Side / Right Side),
   Fill available space (switch off), Show tab icons (slider with five tick
   marks and a trailing value "3"), Use color for tab icons (pop-up value
   "Always"), Shell icon theme (pop-up value "Default").
3. Section "Panes": Pane style (segmented Comfortable / Compact), Show pane
   headers (switch on).

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. The choice tile is a reusable function (preview, label, selected,
   forced state); the selected tile has an accent ring and an emphasized
   label; the two clusters share one group with a vertical divider.
4. Segmented controls come from the kit, use text-only segments of equal
   width (HIG `segmented-controls.md › Content`), and show the selection.
5. The slider row shows tick marks and a trailing value; pop-up rows use the
   kit pop-up value.
6. Kit geometry holds in the capture.
7. The report lists the commands run, the PNG paths and what was not observed.

## S3 — Kagi "General" in Japanese (edge: real strings, long text)

Rebuild Kagi's own General settings pane with the kit, using the real `Msg`
strings so EN and JA both work: sidebar with Kagi's panes (General selected;
Language, Smart Commit, Analyze), page title from `Msg::SettingsGeneral`,
section Appearance (Theme pop-up value "Apple Dark", Accent segmented
System / Theme accent, Toolbar segmented Icon and Text / Icon Only, UI Zoom
stepper 100%, Compact layout threshold stepper 70rem with its description),
section Graph (Compact graph switch off, Avatar commit nodes switch on with
description), section Network (Auto-fetch switch on with description). The
values are static; no setting is written.

Checklist:

1. `[critical]` Host render succeeds for Apple Dark EN, Apple Light JA, and a
   720 pt narrow window in JA.
2. `[critical]` Kit reuse holds.
3. All visible titles and descriptions come from `Msg` (no hard-coded
   English or Japanese sentences in the page).
4. In JA and at 720 pt no text is clipped or overlaps, and long descriptions
   wrap without pushing trailing controls outside the group.
5. The sidebar shows Kagi's panes with icon tiles and exactly one selected.
6. The report lists the commands run, the PNG paths and what was not observed.

## H — Rex "Themes" (hold-out; not used until convergence)

Rebuild Rex's Themes pane (`rex-themes.png`): section "Light Mode" with a
trailing accent action "New"; a group with captions "Default" | "Built-in",
a horizontal row of theme swatch cards (one selected, accent ring, name
underneath), and a switch row "Use extended theme palette"; the same for
"Dark Mode — Active"; section "Import Theme" with a row (title, subtitle) and a
trailing capsule button "Import".

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. The section header's trailing action comes from the kit section accessory.
4. The swatch card is a reusable function with a selected ring; captions are
   separated by a divider as in the reference.
5. The import row uses a kit row and a kit capsule button.
6. The report lists the commands run, the PNG paths and what was not observed.

## Amendment — Apple System Settings scenarios (added before iteration 5)

Added at the owner's request after iteration 4, because S1–S3 saturated at
100 % from iteration 3 and stopped producing signal ("too easy" in the
method). S1–S3 and H are unchanged and keep running; S4/S5 are scored
separately and enter the convergence rule only from iteration 5. References:
`apple-sound.png`, `apple-icloud.png` (owner's captures, Japanese UI; build the
pages with the English labels below).

## S4 — Apple "Sound" (edge: sliders, table, checkbox)

Sidebar (Sound selected; Apple's categories), page title "Sound", and:

1. Section "Sound Effects", one group: Alert sound (pop-up value "Boop"
   followed by a play button), Play sound effects through (pop-up value
   "MacBook Pro Speakers"), Alert volume (slider with a speaker symbol on
   each side and tick marks; near maximum), Play sound on startup (switch
   on), Play user interface sound effects (switch on), Play feedback when
   volume is changed (switch off).
2. Section "Output & Input", one group: a full-width two-choice control
   Output / Input (Output selected); a table with column headers Name and
   Type and six devices (DELL U2723QE / DisplayPort, Shure MV7 / USB,
   MacBook Pro Speakers / Built-in — selected, AirPods Pro / Bluetooth,
   AirMac Express / AirPlay, tomixrm-m1 / AirPlay).
3. A group: Output volume (slider near minimum with speaker symbols, and a
   checked "Mute" checkbox under its trailing end); Balance (centred slider
   with "Left" and "Right" captions under its ends).
4. A help button "?" at the bottom trailing corner.

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. One reusable slider function (value, optional ticks, leading/trailing
   symbol or captions, forced state) draws all three sliders, each thumb at
   its value.
4. The device table is a reusable function: header row, aligned columns,
   exactly one selected row drawn with a theme selection role.
5. The two-choice control spans the group width with the selection visible;
   the Mute checkbox is checked, labelled, and aligned to the slider's
   trailing end.
6. In the Alert sound row the pop-up value and the play button sit together
   at the row's trailing edge, aligned with the other rows' controls.
7. The report lists the commands run, the PNG paths and what was not observed.

## S5 — Apple "iCloud" (edge: cards, progress, value rows)

Sidebar (iCloud selected), page title "iCloud", and:

1. A header: "iCloud+" over "Tomioka Daiki" (large, bold, secondary second
   line) with a round badge symbol at the trailing edge.
2. A storage group: "Storage: 46.5 / 50 GB used", a "Manage…" capsule button
   at the trailing edge, and a full-width progress bar at 93 %.
3. Section "Saved to iCloud" whose header has a leading green status dot and
   a trailing "See All" capsule button; a 3×2 grid of cards (symbol tile,
   title, subtitle): Photos / 111 items, Drive / 35.2 GB, Passwords / On,
   Notes / 706 items, Messages / 921.6 MB, Mail / 677.1 MB.
4. Section "iCloud+ Features": a 3×2 grid of cards: Manage Plan / 50 GB,
   Family / Start sharing, Apple Invitations / Create event, Private Relay /
   On, Hide My Email / 14 addresses, Custom Email Domain / Manage.
5. Two navigation rows with a trailing value before the chevron: Advanced
   Data Protection — Off; Access iCloud Data on the Web — On.
6. A help button "?" at the bottom trailing corner.

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. The card is a reusable function (symbol, title, subtitle, forced state);
   both grids have three equal columns, cards in a row share one height, and
   a two-line title never overlaps its subtitle.
4. The progress bar is a reusable function (fraction, tint) inside the
   storage group, with "Manage…" on the same trailing edge as other
   controls.
5. The two navigation rows show their trailing value before the chevron.
6. The "Saved to iCloud" header shows the status dot and the trailing "See
   All" button.
7. The report lists the commands run, the PNG paths and what was not observed.

## Amendment — Kagi theme cards (added before iteration 6)

Added at the owner's request after iteration 5: Rex shows colour themes as
preview cards and edits them in a sheet (`rex-theme-cards.png`,
`rex-theme-editor.png`). Unlike S1/S2/S4/S5 these pages use **Kagi's real
theme data** (`kagi::ui::theme::themes_by_name()`, each `ThemeHandle`
dereferences to a `Theme` with `bg_base`, `text_main`, `ui_accent()`,
`term_*` and `dark`). Scored from iteration 6.

## S6 — Kagi theme cards (edge: data-driven cards, horizontal strip)

Page "Themes" (sidebar of Kagi panes with a Themes item selected): section
"Light Mode" and section "Dark Mode — Active", each with a trailing "New"
section action and one group containing a "Built-in" caption, a horizontal
strip of theme cards for every theme of that mode (from `themes_by_name()`,
split by `dark`), and a switch row "Use extended theme palette" (on). A card
is a preview tile painted in **that theme's own colours** — `bg_base`
background, "Aa" in `text_main`, a "~/" chip and a cursor block in its
terminal/accent colours, a row of its eight ANSI colours as dots — with the
theme's `display_name()` underneath. Apple Light is selected in Light Mode,
Apple Dark in Dark Mode (static).

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. The card is a reusable function (theme, selected, forced state) whose
   preview colours come only from that theme's fields — the previews look
   the same under Apple Dark and Apple Light; no hex literals.
4. Every built-in theme appears exactly once, in the section matching its
   `dark` flag, in `themes_by_name()` order.
5. The selected card has the accent ring and an emphasized name; the strip
   scrolls horizontally (stable element id) instead of squeezing cards.
6. The report lists the commands run, the PNG paths and what was not observed.

## S7 — Theme editor sheet (edge: sheet, text field, swatches, footer)

Over the S6-style Themes page (dimmed), a sheet: a palette symbol, caption
"New Theme Name" above a text field showing "Apple Dark Copy", a trailing
"Dark Theme" pill; section "Colors" with a trailing "Preview" switch (on);
groups "Terminal" (Background, Text, Cursor, Cursor Text, Selection, Selected
Text), "ANSI Standard" and "ANSI Bright" (Black … White) — each entry a
colour swatch with its label above and its hex value below, all taken from
Apple Dark's `term_*` fields; a full-width "Show extended palette" button;
a footer with "Revert…" at the leading edge and "Cancel" + "Save & Apply"
(the one primary button) at the trailing edge.

Checklist:

1. `[critical]` Host render succeeds for Apple Dark and Apple Light (EN).
2. `[critical]` Kit reuse holds.
3. The swatch is a reusable function (colour, label, forced state) and its
   hex text is computed from the theme value; swatches line up in columns
   within each group.
4. The name field is a reusable text-field look with a visible focus state
   available as a named state.
5. The footer has exactly one primary button, Cancel immediately before it,
   and Revert… separated at the leading edge (HIG `alerts.md › Buttons`).
6. The page behind the sheet is dimmed and the sheet has its own rounded
   surface and shadow.
7. The report lists the commands run, the PNG paths and what was not observed.

## Amendment — placement (added before iteration 7)

Added at the owner's request after iteration 6: Kagi's General pane mixes
Graph, Network and Behaviour settings that could be sidebar panes. S1–S7
test how parts look; S8 tests where settings go. Scored from iteration 7.

## S8 — Kagi Settings information architecture (edge: placement)

Reorganize all of Kagi's settings into a sidebar of panes and build the
result as a gallery page. Today's settings (current pane → setting, control):

- General / Appearance: Theme (pop-up), Accent (System | Theme accent),
  Custom themes (folder path, Open theme folder…, Reload Themes, Theme file
  guide), Toolbar (Icon and Text | Icon Only), UI Zoom (stepper 70–200%),
  Compact layout threshold (stepper 40–160rem).
- General / Graph: Compact graph (switch), Avatar commit nodes (switch).
- General / Network: Auto-fetch (switch).
- General / Behaviour / Terminal: Graph ⌘C copies (Hash | Branch),
  Markdown opening mode (Preview | Source), Reduce motion (switch), Lock
  worktree while the terminal is open (switch).
- Language: Language (English | 日本語).
- Smart Commit: enable (switch), provider (Ollama / CLI providers), model
  (detected models).
- Analyze: ignore list (multi-line text, Save, Reset…).

The page shows the full sidebar and renders each pane as a named state
(`pane-<name>`), the first pane as `default`. Use `Msg` strings where they
exist; new pane titles may be English literals.

Checklist:

1. `[critical]` Host render succeeds for every pane state in Apple Dark and
   for the default pane in Apple Light.
2. `[critical]` Every setting above appears in exactly one pane, none is
   dropped (including Custom themes' three actions and Analyze's Reset…).
3. General holds only app-wide basics; Graph, Network and Behaviour items
   live in panes named for their domain.
4. The report contains a placement table (setting → pane → section) with a
   one-line reason per pane.
5. Pane order puts the most frequently changed panes first; each pane has at
   most two levels (pane → section).
6. Kit reuse holds and sidebar tiles use category hues.
7. The report lists the commands run, the PNG paths and what was not observed.
