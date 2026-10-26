---
name: kagi-settings-page
description: Build a Kagi settings page (or a settings-like grouped form) as a native gallery page composed from the Kagi-owned `settings_kit` components, and prove it with gallery captures. Use when asked to rebuild a reference app's settings screen in Kagi, prototype a settings pane, or add a settings control the kit lacks.
---

# Building a Kagi settings page

Version: v1 (experiment `experiments/settings-skill`).

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
`KAGI_GALLERY_SIZE=720x692` when text width matters. Run `rustfmt` on your
file only.

## 5. Critique the captures before reporting

"It looks like the reference" is not a check. Open each PNG and answer every
line below with what you see; fix and recapture until each is true. Write
findings as `[C1]` (breaks a kit or §3 rule — must be fixed) or `[C2]`
(differs from a sibling part of the same role — fix or justify).

- **Indicators sit on their value.** A thumb, check, ring or fill is drawn
  inside its own track or cell, at the position the value says.
- **Same role, same look.** Every control of one kind (switches, segmented,
  pop-ups, tiles, page-local parts) has one height, radius and fill; a
  page-local part matches the kit parts around it.
- **One trailing edge.** All trailing controls end on the same x.
- **§3 beats the reference.** Where the reference image breaks a §3 rule
  (for example multi-coloured sidebar tiles), follow §3 and say so.
- **Light and dark both read.** No part disappears or loses its border in
  either theme.

## 6. Report

Commands run, PNG paths, each requirement ○ / × / partial with the reason,
your `[C1]` / `[C2]` findings and what you changed for them, kit gaps, and
what you did not observe (real hover, keyboard focus, VoiceOver).
