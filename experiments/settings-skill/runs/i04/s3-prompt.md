You are a blank-slate implementer of the skill `kagi-settings-page`. Follow the skill below; it is the only instruction about how to build the page.

Repository (shared checkout, Rust/GPUI): /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill
Write ONLY the new file `examples/lab_i04_s3.rs` (gallery page name `lab-i04-s3`). Do not modify or create any other file, do not read `experiments/` or other `examples/lab_*` files, and do not run `cargo fmt --all`, tests or clippy (use `rustfmt --edition 2021 examples/lab_i04_s3.rs`). Other implementers build at the same time: if cargo prints "Blocking waiting for file lock", wait. Never take the foreground or activate an app.
Capture settings for this machine: KAGI_GALLERY_DISPLAY=ad1ab626 (a visible external display). Save your PNGs under /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i04/s3/.
Reference for the current Kagi General pane (open it): /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/shots/baseline/kagi-settings-general-en-dark.png

## Skill (verbatim)
---
name: kagi-settings-page
description: Build a Kagi settings page (or a settings-like grouped form) as a native gallery page composed from the Kagi-owned `settings_kit` components, and prove it with gallery captures. Use when asked to rebuild a reference app's settings screen in Kagi, prototype a settings pane, or add a settings control the kit lacks.
---

# Building a Kagi settings page

Version: v3 (experiment `experiments/settings-skill`).

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
    size: (900., 692.),                   // logical window size at 1.0×
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

## 2. The kit (`src/ui/settings_kit.rs`)

| Part | Call | Notes |
| --- | --- | --- |
| Window | `settings_window(sidebar, detail)` | sidebar leading, detail trailing |
| Sidebar | `sidebar([AnyElement…])`, `sidebar_gap()` | inset panel; leave the top for traffic lights |
| Search | `search_field(id, placeholder)` | look only |
| Sidebar row | `sidebar_item(id, tile, label, subtitle: Option<SharedString>, selected, forced)` | exactly one selected; its tile is **always `Tint::Accent`**, whatever colours the reference uses |
| Icon tile | `icon_tile(IconName, Tint, size)` | 20 in the sidebar, 26–32 in rows; a non-accent `Tint` only for a status the row reports (`Success` done, `Warning` blocked, `Danger` error) |
| Page | `detail_page(scroll_id, title, [sections…])` | title = pane name |
| Section | `section(title, subtitle, accessory, body)` | returns `AnyElement`; accessory e.g. `section_action` |
| Section action | `section_action(id, IconName, label, forced)` | accent text button ("New") |
| Group | `group([rows…])` | card; inserts hairlines between rows |
| Row | `Row::new(title).subtitle(..).leading(..).trailing(..)` | `.navigation(id, forced)` adds chevron + press |
| Switch | `switch(id, label, checked, forced, on_toggle)` | mini switch |
| Segmented | `Segmented::new(id, labels, selected).forced(i, state)` | 2–5 **text** choices (nouns), equal widths; never icons. Choices shown as visual previews are a page-local choice tile instead |
| Pop-up value | `popup_value(id, value, forced)` | value + up/down chevrons |
| Stepper | `stepper(id, value, can_dec, can_inc, forced)` | `forced: Option<(StepperPart, Forced)>` |
| Capsule button | `capsule_button(id, label, forced)` | neutral row action |
| Divider / caption | `vertical_divider()`, `caption(text)` | clusters inside one group |
| Ring | `ring(element, on, radius)` | 2 px accent focus/selection ring |
| Sizes / colours | `pt(n)`, `metrics::*`, `ink::*`, `Tint` | never hard-code px or hex |

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

If the kit has no part for a control the page needs (for example a slider),
write it **in your page file** as a function with explicit parameters (value,
selected, `forced`), using only `pt`, `metrics`, `ink`, `ring` and `Tint`, and
name it as a *kit gap* in your report. Do not edit `src/` files.

## 3. Apple HIG rules the kit cannot enforce

The rules for tiles and segmented controls are in the table above, next to
the part, because a reference image is consulted part by part and wins over a
rule read later. They apply even where the reference does otherwise.

- A long or open list of values → `popup_value`.
- A button that opens another window or app ends with "…".
- A chevron row only for navigation inside the settings window.

## 4. Verify

```sh
cargo build --example <name>
target/debug/examples/<name> --list
KAGI_GALLERY_DISPLAY=<uuid-prefix> KAGI_GALLERY_THEME=apple-dark \
  KAGI_GALLERY_STATE=default KAGI_GALLERY_OUT=<dir>/<name>-default-dark.png \
  target/debug/examples/<name>
```

Repeat for `apple-light`, every state, and `KAGI_GALLERY_LANG=ja` /
`KAGI_GALLERY_SIZE=720x692` when text width matters. Run `rustfmt` on your
file only.

## 5. Critique the captures before reporting

"It looks like the reference" is not a check. Open each PNG and answer every
line below with what you see; fix and recapture until each is true. Write
findings as `[C1]` (breaks a §2 table or §3 rule — must be fixed) or `[C2]`
(differs from a sibling part of the same role — fix or justify).

- **Indicators sit on their value.** A thumb, check, ring or fill is drawn
  inside its own track or cell, at the position the value says.
- **Same role, same look.** Every control of one kind (switches, segmented,
  pop-ups, tiles, page-local parts) has one height, radius and fill; a
  page-local part matches the kit parts around it.
- **One trailing edge.** All trailing controls end on the same x.
- **The skill beats the reference.** Where the reference image breaks a §2
  table or §3 rule (sidebar tile colours, icon segments), follow the skill.
- **Light and dark both read.** No part disappears or loses its border in
  either theme.

## 6. Report

Commands run, PNG paths, each requirement ○ / × / partial with the reason,
your `[C1]` / `[C2]` findings and what you changed for them, kit gaps, and
what you did not observe (real hover, keyboard focus, VoiceOver).


## Scenario
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

Common definitions:
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

## Task
1. Implement the scenario following the skill.
2. Verify as the skill says.
3. Reply with the report below.

## Report (reply in English, this exact structure)
- Deliverable / evidence: file path, commands run, PNG paths
- Requirement self-check: each checklist item ○ / × / partial with the reason
- Unclear points: each as `[phase] Issue / Cause / General Fix Rule`, phase one of Understanding, Planning, Execution, Formatting (write "none" if none)
- Discretionary fills: decisions the skill did not determine
- Kit gaps: page-local components you had to write
- Retries: count and reason
- Not observed: what you could not verify
