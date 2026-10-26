---
name: kagi-settings-page
description: Build a Kagi settings page (or a settings-like grouped form) as a native gallery page composed from the Kagi-owned `settings_kit` components, and prove it with gallery captures. Use when asked to rebuild a reference app's settings screen in Kagi, prototype a settings pane, or add a settings control the kit lacks.
---

# Building a Kagi settings page

Version: v9 (experiment `experiments/settings-skill`).

A settings page is **composed**, not drawn. Every row, group, switch,
segmented control, pop-up value, stepper, navigation row, icon tile, sidebar
item and capsule button comes from `kagi::ui::settings_kit` (one owner per
control kind, #1800). You write layout and content; the kit owns geometry and
colour.

## 1. Where the page lives

A page is a gallery target in its own example file, `examples/<name>.rs`:

```rust
use gpui::{prelude::*, AnyElement, App, Window};
use gpui_component::IconName;
use kagi::ui::gallery_host::{self, GalleryContext, GalleryPage};
use kagi::ui::settings_kit::*;

static PAGES: [GalleryPage; 1] = [GalleryPage {
    name: "my-page",
    states: &["default", "focus-switch"], // first = default; others = `<interaction>-<part>`
    size: (900., 692.),                   // logical size at 1.0×: grow the height until no row is cut off
    render,
}];

fn main() {
    gallery_host::run(&PAGES);
}

fn render(cx: &GalleryContext, _: &mut Window, _: &mut App) -> AnyElement {
    let focus = if cx.state == "focus-switch" { Forced::Focus } else { Forced::Rest };
    settings_window(
        sidebar([
            search_field("search", "Search Settings").into_any_element(),
            sidebar_item("general", icon_tile(IconName::Settings2, Tint::Accent, 20.),
                "General", None, true, Forced::Rest).into_any_element(),
        ]),
        detail_page("scroll", "General", [
            section("Appearance", None, None, group([
                Row::new("Show icons")
                    .trailing(switch("icons", "Show icons", true, focus, |_, _, _| {}))
                    .into_any_element(),
            ])),
        ]),
    )
    .into_any_element()
}
```

The complete worked example is `examples/settings_gallery.rs` (`server` page
= Rex's Server pane; `parts` page = every part in every forced state). Read it
before writing yours.

## 2. The kit (`src/ui/settings_kit/`)

| Part | Call | Notes |
| --- | --- | --- |
| Window | `settings_window(sidebar, detail)` | sidebar leading, detail trailing |
| Sidebar | `sidebar([AnyElement…])`, `sidebar_gap()` | inset panel; leave the top for traffic lights |
| Search | `search_field(id, placeholder)` | look only |
| Sidebar row | `sidebar_item(id, tile, label, subtitle: Option<SharedString>, selected, forced)` | exactly one selected |
| Icon tile | `icon_tile(IconName, Tint, size)` | 20 in the sidebar, 26–32 in rows and cards. **Every tile — sidebar, row or card — uses a category colour `Tint::Hue(Hue::…)` from the hue table below**, never one colour for everything. Theme status tints (`Success` done, `Warning` blocked, `Danger` error) only for a status that row reports |
| Page | `detail_page(scroll_id, title, [sections…])` | title = pane name |
| Section | `section(title, subtitle, accessory, body)` | returns `AnyElement`; accessory e.g. `section_action` |
| Section action | `section_action(id, IconName, label, forced)` | accent text button ("New") |
| Group | `group([rows…])` | card; inserts hairlines between rows |
| Row | `Row::new(title).subtitle(..).leading(..).trailing(..)` | `.navigation(id, forced)` adds chevron + press |
| Switch | `switch(id, label, checked, forced, on_toggle)` | mini switch |
| Segmented | `Segmented::new(id, labels, selected).forced(i, state)`, `.full_width()` to span a group | 2–5 **text** choices (nouns), equal widths; never icons. Choices shown as visual previews are a page-local choice tile instead |
| Pop-up value | `popup_value(id, value, forced)` | value + up/down chevrons |
| Stepper | `stepper(id, value, can_dec, can_inc, forced)` | `forced: Option<(StepperPart, Forced)>` |
| Capsule button | `capsule_button(id, label, forced)` | neutral row action |
| Slider | `Slider::new(id, fraction).ticks(n).ends(SliderEnds::Symbols(a, b) \| Captions(l, r)).width(pt).forced(s)` | fill, thumb and ticks share one value→x mapping |
| Checkbox | `checkbox(id, label, checked, forced)` | label on the trailing side |
| Progress | `progress_bar(fraction, tint)` | fills its container's width |
| Divider / caption | `vertical_divider()`, `caption(text)` | clusters inside one group |
| Ring | `ring(element, on, radius)` | 2 px accent focus/selection ring |
| Sizes / colours | `pt(n)`, `metrics::*`, `ink::*`, `Tint`, `Hue` | never hard-code px or hex |

Hue table (macOS System Settings palette; pick by what the destination is,
and avoid giving neighbouring sidebar items the same hue when another fits):

| Hue | For |
| --- | --- |
| `Gray` | general, keyboard, terminal, developer, security, passwords |
| `Blue` | display, network, remote, cloud, accessibility, language |
| `Indigo` | appearance, themes, focus, time |
| `Purple` | AI, automation, assistants, servers |
| `Pink` | sound, media |
| `Red` | notifications, alerts |
| `Orange` | git, integrations, sharing |
| `Yellow` | notes, favourites, highlights |
| `Green` | battery, health, running, messages |
| `Teal` | storage, data, analysis, files |

`IconName` has exactly these variants (any other name fails to compile):
`ALargeSmall`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `ArrowUp`, `Asterisk`,
`Battery`, `BatteryCharging`, `BatteryFull`, `BatteryLow`, `BatteryMedium`,
`BatteryWarning`, `Bell`, `BookOpen`, `Bot`, `Building2`, `Calendar`,
`CaseSensitive`, `ChartPie`, `Check`, `ChevronDown`, `ChevronLeft`,
`ChevronRight`, `ChevronUp`, `ChevronsUpDown`, `CircleCheck`, `CircleUser`,
`CircleX`, `Close`, `Copy`, `Cpu`, `Dash`, `Delete`, `Ellipsis`,
`EllipsisVertical`, `ExternalLink`, `Eye`, `EyeOff`, `File`, `Folder`,
`FolderClosed`, `FolderOpen`, `Frame`, `GalleryVerticalEnd`, `Github`,
`Globe`, `HardDrive`, `Heart`, `HeartOff`, `Inbox`, `Info`, `Inspector`,
`LayoutDashboard`, `Loader`, `LoaderCircle`, `Map`, `Maximize`,
`MemoryStick`, `Menu`, `Minimize`, `Minus`, `Moon`, `Network`, `Palette`,
`PanelBottom`, `PanelBottomOpen`, `PanelLeft`, `PanelLeftClose`,
`PanelLeftOpen`, `PanelRight`, `PanelRightClose`, `PanelRightOpen`, `Pause`,
`Play`, `Plus`, `Redo`, `Redo2`, `Replace`, `ResizeCorner`, `Search`,
`Settings`, `Settings2`, `SortAscending`, `SortDescending`, `SquareTerminal`,
`Star`, `StarFill`, `StarOff`, `Sun`, `ThumbsDown`, `ThumbsUp`,
`TriangleAlert`, `Undo`, `Undo2`, `User`, `WindowClose`, `WindowMaximize`,
`WindowMinimize`, `WindowRestore`. Pick the nearest one; never guess.

`Forced` (`Rest`, `Hover`, `Pressed`, `Focus`, `Disabled`) draws an
interaction state for captures; background windows never receive real hover
or focus.

**Interaction states never change data.** A state named for an interaction
(`rest`, `hover`, `pressed`, `focus`, `disabled`, e.g. `focus-switch`,
`hover-nav`) is the default page plus that one forced interaction on one
part; every value, selection and text stays as in `default` — "rest" means
"not hovered or pressed", not "nothing selected". To show another *value*,
add a state named for the value (`selected-top-left`) and change everything
that describes it together (the row's subtitle follows the selected cell).

If the kit has no part for a control the page needs, write it **in your
page file** as a function with explicit parameters (value, selected,
`forced`), using only `pt`, `metrics`, `ink`, `ring`, `Tint` and `Hue`, and
name it as a *kit gap* in your report. Do not edit `src/` files. Two rules
for page-local parts:

- Place every indicator (thumb, tick, check, selection) through one
  value → position function, and check it at the minimum, middle and maximum.
- A wrapper around kit parts keeps `w_full()` and `min_w_0()`; a heading or
  row that collapses inside your wrapper is missing them.
- A page-local control that sits in a row next to kit controls is
  `metrics::CONTROL_HEIGHT` tall, like every kit control.

## 3. Decide placement before composing

Where a setting lives matters as much as how it looks. Before writing rows,
write a placement table (setting → pane → section) and put it in your
report.

- **One sidebar pane per domain the user thinks in** — the *object* a
  setting changes: Appearance (themes, accent, zoom, toolbar), Graph,
  Editor, Terminal, Network & Sync, Smart Commit… A section heading that
  names a domain and holds two or more settings is a pane, not a section of
  General.
- **Never a grab-bag pane.** "Behaviour", "Advanced", "Misc", "Other" name
  a *kind* of setting, not an object; move each item to the pane of what it
  affects (Graph ⌘C copies → Graph; Markdown opening mode → Editor; lock
  worktree while the terminal is open → Terminal).
- **General holds only app-wide basics** that fit no domain (language,
  startup, updates). A one-setting domain becomes a section of General or of
  its nearest pane, not a pane of its own. If General has more sections than
  another pane, split it.
- **Group by what the setting affects, not by control type**; every setting
  appears exactly once (HIG `layout.md` › "Group related items").
- **Order panes and sections by how often people change them**, most common
  first; rarely changed or risky settings go last (HIG `settings.md`:
  general, infrequently changed settings belong in the settings window).
- **No more than two levels**: pane → section. Deeper detail is a
  navigation row inside the pane (HIG `sidebars.md`).
- Pane names are short nouns; the page title repeats the sidebar label.
- **When the task fixes the layout** (rebuild this pane as it is), keep the
  given layout and put your placement table under "Placement suggestions"
  in the report instead of moving settings.

## 4. Apple HIG rules the kit cannot enforce

The rules for tiles and segmented controls are in the table above, next to
the part, because a reference image is consulted part by part and wins over a
rule read later. They apply even where the reference does otherwise.

- A long or open list of values → `popup_value`.
- A button that opens another window or app ends with "…".
- A chevron row only for navigation inside the settings window.
- A row's control never repeats the row's title: the title names the
  object, the control names the action or value ("Theme folder" —
  "Open…"; "Custom themes" — "Reload"). A row whose only content is an
  action is a single capsule in the section, not a title plus a button.

## 5. Verify

```sh
cargo build --example <name> --features gui-e2e
target/debug/examples/<name> --list
KAGI_GALLERY_THEME=apple-dark KAGI_GALLERY_STATE=default \
  KAGI_GALLERY_OUT=<dir>/<name>-default-dark.png target/debug/examples/<name>
```

With `--features gui-e2e` the host renders the page offscreen through Metal
(no window appears, nothing depends on which display is visible); always
build with it so every build shares one cache. Repeat for `apple-light`,
every state, and `KAGI_GALLERY_LANG=ja` / `KAGI_GALLERY_SIZE=720x692` when
text width matters. Run `rustfmt` on your file only.

**A capture is evidence only for what it shows.** If the default capture cuts
off a row the scenario names, raise the page's `size` height and recapture.
An item you could not see in any capture is reported as partial with "not
captured", never ○.

## 6. Critique the captures before reporting

"It looks like the reference" is not a check. Open each PNG and answer every
line below with what you see; fix and recapture until each is true. Write
findings as `[C1]` (breaks a §2 table, §3 placement or §4 rule — must be fixed) or `[C2]`
(differs from a sibling part of the same role — fix or justify).

- **Indicators sit on their value.** A thumb, check, ring or fill is drawn
  inside its own track or cell, at the position the value says.
- **Same role, same look.** Every control of one kind (switches, segmented,
  pop-ups, tiles, page-local parts) has one height, radius and fill; a
  page-local part matches the kit parts around it.
- **One trailing edge.** All trailing controls end on the same x.
- **Same row, same height.** Things placed side by side share one height and
  one centre line: a pop-up and its button, segments, swatches, cards in a
  grid row (all as tall as the tallest), capsules in a button row. Measure it
  in the PNG; a 2 px difference is a finding.
- **The skill beats the reference.** Where the reference image breaks a §2
  table or §4 rule (sidebar tile colours, icon segments), follow the skill.
- **Light and dark both read.** No part disappears or loses its border in
  either theme.

## 7. Report

Commands run, PNG paths, each requirement ○ / × / partial with the reason,
your `[C1]` / `[C2]` findings and what you changed for them, kit gaps, and
what you did not observe (real hover, keyboard focus, VoiceOver).
