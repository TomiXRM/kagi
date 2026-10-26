You are a blank-slate implementer of the skill `kagi-settings-page`. Follow the skill below; it is the only instruction about how to build the page.

Repository (shared checkout, Rust/GPUI): /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill
Write ONLY the new file `examples/lab_i01_s2.rs` (gallery page name `lab-i01-s2`). Do not modify or create any other file, do not read `experiments/` or other `examples/lab_*` files, and do not run `cargo fmt --all`, tests or clippy (use `rustfmt --edition 2021 examples/lab_i01_s2.rs`). Other implementers build at the same time: if cargo prints "Blocking waiting for file lock", wait. Never take the foreground or activate an app.
Capture settings for this machine: KAGI_GALLERY_DISPLAY=ad1ab626 (a visible external display). Save your PNGs under /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s2/.
Reference image (open it): /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/reference/rex-interface.png

## Skill (verbatim)
---
name: kagi-settings-page
description: Build a Kagi settings page (or a settings-like grouped form) as a native gallery page composed from the Kagi-owned `settings_kit` components, and prove it with gallery captures. Use when asked to rebuild a reference app's settings screen in Kagi, prototype a settings pane, or add a settings control the kit lacks.
---

# Building a Kagi settings page

Version: v0 (experiment `experiments/settings-skill`).

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
    states: &["default", "focus-switch"], // first = default; every state must look different
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
| Sidebar row | `sidebar_item(id, tile, label, subtitle: Option<SharedString>, selected, forced)` | exactly one selected |
| Icon tile | `icon_tile(IconName, Tint, size)` | 20 in the sidebar, 26–32 in rows |
| Page | `detail_page(scroll_id, title, [sections…])` | title = pane name |
| Section | `section(title, subtitle, accessory, body)` | returns `AnyElement`; accessory e.g. `section_action` |
| Section action | `section_action(id, IconName, label, forced)` | accent text button ("New") |
| Group | `group([rows…])` | card; inserts hairlines between rows |
| Row | `Row::new(title).subtitle(..).leading(..).trailing(..)` | `.navigation(id, forced)` adds chevron + press |
| Switch | `switch(id, label, checked, forced, on_toggle)` | mini switch |
| Segmented | `Segmented::new(id, labels, selected).forced(i, state)` | text-only, equal widths |
| Pop-up value | `popup_value(id, value, forced)` | value + up/down chevrons |
| Stepper | `stepper(id, value, can_dec, can_inc, forced)` | `forced: Option<(StepperPart, Forced)>` |
| Capsule button | `capsule_button(id, label, forced)` | neutral row action |
| Divider / caption | `vertical_divider()`, `caption(text)` | clusters inside one group |
| Ring | `ring(element, on, radius)` | 2 px accent focus/selection ring |
| Sizes / colours | `pt(n)`, `metrics::*`, `ink::*`, `Tint` | never hard-code px or hex |

`Forced` (`Rest`, `Hover`, `Pressed`, `Focus`, `Disabled`) draws an
interaction state for captures; background windows never receive real hover
or focus.

If the kit has no part for a control the page needs (for example a slider),
write it **in your page file** as a function with explicit parameters (value,
selected, `forced`), using only `pt`, `metrics`, `ink`, `ring` and `Tint`, and
name it as a *kit gap* in your report. Do not edit `src/` files.

## 3. Apple HIG rules the kit cannot enforce

- One choice among 2–5 closely related values → `Segmented` with text-only
  labels (nouns). Never mix icons and text in one segmented control.
- A long or open list of values → `popup_value`.
- A button that opens another window or app ends with "…".
- A chevron row only for navigation inside the settings window.
- Sidebar tiles use `Tint::Accent` unless a colour carries meaning (status:
  `Success`, `Warning`, `Danger`).

## 4. Verify

```sh
cargo build --example <name>
target/debug/examples/<name> --list
KAGI_GALLERY_DISPLAY=<uuid-prefix> KAGI_GALLERY_THEME=apple-dark \
  KAGI_GALLERY_STATE=default KAGI_GALLERY_OUT=<dir>/<name>-default-dark.png \
  target/debug/examples/<name>
```

Repeat for `apple-light`, every state, and `KAGI_GALLERY_LANG=ja` /
`KAGI_GALLERY_SIZE=720x692` when text width matters. Open every PNG and check
it against the reference before reporting. Run `rustfmt` on your file only.

## 5. Report

Commands run, PNG paths, each requirement ○ / × / partial with the reason,
kit gaps, and what you did not observe (real hover, keyboard focus, VoiceOver).


## Scenario
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
