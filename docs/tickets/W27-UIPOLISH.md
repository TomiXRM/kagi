# W27-UIPOLISH — Compact context menus + window zoom

Status: **done** (build + `cargo test --workspace` green, 574 tests pass; menu
dump verified; zoom persistence round-trip verified).

## Scope

Two user requests:

1. **Tighter context menus (Zed-style density).** Reduce the vertical spacing of
   the commit context menu, branch context menu, and the file context menu so
   items read as a compact list rather than a loose one.
2. **Window zoom** with `Cmd+-` / `Cmd+=` (+) and `Cmd+0` (reset), wired through
   the command registry + View menu, clamped `0.7..=1.5`, persisted in
   `settings.json` under `"ui_zoom"`.

## 1. Compact menus

The commit (`src/ui/context_menu.rs`) and branch (`src/ui/branch_menu.rs`) menus
share an identical layout grammar (header bar, group title, item row). Tightened
the shared density constants in both:

| const          | before | after |
|----------------|--------|-------|
| `MENU_ROW_H`   | 28     | 24    |
| `MENU_HEADER_H`| 36     | 30    |
| `MENU_GROUP_H` | 22     | 18    |

Group titles went `pt_2` → `pt_1`. Hover highlight, disabled styling
(`text_muted` / `color_blocker_muted` + tooltip reason), group separators, and
`text_sm`/`text_xs` consistency are all preserved.

The file context menu (`render_file_menu_overlay` in `src/ui/mod.rs`) is a
single-item popover; its panel/item vertical padding was tightened to `py(px(2))`
/ `py(px(3))` to match.

## 2. Window zoom — mechanism investigation & choice

**Chosen mechanism: option (a) — `window.set_rem_size()` rem-size scaling.**

Evidence (gpui 0.2.2 at
`~/.cargo/registry/src/*/gpui-0.2.2/`):

- `Window::set_rem_size(impl Into<Pixels>)` exists (window.rs:1830), default
  rem size is `px(16.)` (window.rs:1215), and `rem_size()` is consulted by div /
  uniform_list / list / img layout + paint every frame.
- gpui's Tailwind text helpers resolve through rem: `text_xs = rems(0.75)`,
  `text_sm = rems(0.875)`, `text_base = rems(1.0)`, etc. (styled.rs:433+), and
  rems → pixels via `rem_size()`.
- kagi uses `text_sm`/`text_xs`/`text_lg`/… **260+ times** and explicit
  `.text_size(px(..))` only **twice** (inspector avatar initial + one header).

So scaling rem size zooms virtually all kagi text like a web-page zoom. Tested:
the app renders correctly with a scaled rem size (no panic; layout reflows),
and `ui_zoom: "1300"` in settings → `[kagi] zoom: 1.30x` at startup, clamped to
`[0.7, 1.5]`.

**Honest limitation:** dimensions written as literal `px(..)` (row heights,
panel widths, the two explicit `.text_size(px(9.))` call-sites, the menu
constants above) do **not** scale — they are absolute. This is the same
trade-off Zed accepts; the dominant user-visible effect (all body/label text)
scales, which is what the request asked for. A future pass could migrate
hot-path `px(..)` sizes to `rems(..)` for fully proportional zoom, but that is
out of scope here and was not faked.

### Implementation

- `src/ui/theme.rs`: global `UI_ZOOM_PERMILLE: AtomicUsize` (mirrors the
  `ACTIVE` theme atomic — survives tabs/window re-create). Helpers `zoom()`,
  `set_zoom()` (clamps + persists), `init_zoom()`, `rem_size_px()`,
  `clamp_zoom()`, consts `BASE_REM_PX=16`, `ZOOM_MIN=0.7`, `ZOOM_MAX=1.5`,
  `ZOOM_STEP=0.1`. Zoom stored as permille integer so it fits an atomic int.
  Added `"ui_zoom"` to `SETTINGS_KEYS`.
- `src/ui/mod.rs` `Render::render`: `window.set_rem_size(px(theme::rem_size_px()))`
  at the top — re-asserted every frame so it self-heals after zoom changes /
  window re-create.
- `src/ui/commands.rs`: `view.zoomIn/Out/Reset` made always-`Enabled`; keystrokes
  `cmd-=` / `cmd--` / `cmd-0` in `COMMANDS`; `KeyBinding`s registered (also
  `cmd-+` as an alias for zoom-in); `handle_menu_command` arms mutate
  `theme::set_zoom(zoom ± step | 1.0)`.
- `src/main.rs`: `theme::init_zoom()` at startup (after `init_active`).
- `src/ui/i18n.rs`: removed the now-dead `Msg::ZoomUnimplemented` variant
  (would otherwise be an unused-variant warning).

## Verification

- `cargo build` — green, 0 own-code warnings (only the unrelated `block v0.1.6`
  future-incompat note from a dependency).
- `cargo test --workspace` — exit 0, 574 passed / 0 failed. Added unit tests in
  theme.rs (`zoom_clamps_to_bounds`, `ui_zoom_in_settings_keys`).
- `KAGI_MENU_DUMP=1` — all three zoom commands `state=enabled` with the expected
  keystrokes.
- Settings round-trip: `ui_zoom: "1300"` → `[kagi] zoom: 1.30x` at startup.

## Hard-rule compliance

- New string handling uses `chars()` (header truncation already did); no new
  `split_at`/byte slicing introduced. (Existing `parse_string_value` byte-slices
  but is untouched.)
- Colours via `theme()`; prose via i18n `Msg` (zoom commands have no new prose —
  labels live in `COMMANDS`, matching the existing menu pattern).
- No `Cargo.toml` / vendor changes.
