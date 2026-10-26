# Importing the experiment into Kagi

What this branch adds and what an import PR must still do. Nothing here
changes the production Settings window.

## Added

| Path | Role |
| --- | --- |
| `src/ui/settings_kit/mod.rs` | Single public owner of settings parts: window scaffold, sidebar, sidebar item, icon tile (`Tint`, category `Hue`), page, section (+ action), group, `Row` (subtitle / leading / trailing / navigation), `Forced`, `metrics` (one `CONTROL_HEIGHT`), `ink`. |
| `src/ui/settings_kit/controls.rs` | Switch, `Segmented` (`.full_width()`), pop-up value, stepper, capsule button, `Slider` (`SliderEnds`), checkbox, progress bar. |
| `src/ui/gallery_host.rs` | Native gallery host: named page/state, theme, language, zoom, size, capture-and-exit, `--list`, failure on unknown names. With `--features gui-e2e` captures render offscreen through Metal; without it, `screencapture` of a visible window that refuses occluded windows. |
| `examples/settings_gallery.rs` | Reference gallery: `parts` (every part × forced state) and `server` (worked example page). |
| `experiments/settings-skill/` | Protocol, fixed scenarios (S1–S8, H), skill v0→v9 snapshots, per-iteration runs (prompts, reports, child sources as `.txt`, scores, capture manifests), ledger, PDCA log, HIG reviews. |

## Candidates the experiment produced

- **Skill**: `experiments/settings-skill/skill/SKILL.md` (v9) → copy to
  `.claude/skills/settings-page/` (and `.agents/skills/kagi-settings-page`)
  when the kit reaches production.
- **Kit promotions**: page-local parts that every run rebuilt — choice tile
  (S2), theme card driven by `Theme` (S6, also the owner's request), colour
  swatch and text-field look (S7), table (S4), card grid (S5), position
  picker (S1). A theme card that draws only from `ThemeHandle` fields is the
  first candidate for the production Appearance pane.
- **Placement**: S8's runs converged on Appearance / Graph / Editor /
  Terminal / Network & Sync / Smart Commit / Analyze / General (language) —
  the starting proposal for splitting today's General pane.

## Debt with removal conditions

1. **Two owners of settings rows.** `settings_view.rs` keeps
   `section_header`, `settings_group`, `setting_row`, `setting_row_with_help`.
   Removed when the production Settings window renders through
   `settings_kit`; the Tier A contracts that read those helpers
   (`settings_quiet`, `settings_groups`, `settings_switches`,
   `settings_window_screen_fit`) are updated in the same PR with the owner's
   design approval, not loosened.
2. **Kit controls without keyboard wiring.** `Segmented`, `popup_value`,
   `stepper`, `capsule_button` and navigation rows draw focus only when
   `Forced::Focus` is passed. At import they take their Tab stops from
   `keyboard_nav` (`TabList` for segmented, `focusable` / `switch` for the
   rest); the production switch stays `keyboard_nav::switch`.
3. **Gallery outside inventory.** Gallery targets are not yet
   `inventory:gallery:<part>` entries (#1799). Moving them needs the inventory
   supervisor to accept a window without `KagiApp`.
4. **No CI gate yet.** #1800's `shared-input`-style `Rule` for settings
   parts (e.g. `Switch::new` outside `settings_kit.rs` / `keyboard_nav.rs`) is
   added once production uses the kit; before that a ratchet would only count
   the existing settings helpers.
5. **Icon catalog gaps.** No speaker, cloud or photo symbols; children used
   the nearest names. Add Kagi-owned SVGs before Sound-like panes ship.
6. **Feature-flag capture.** Offscreen capture needs `--features gui-e2e`,
   which also replaces `target/debug/kagi` with a harness build (copy a plain
   binary first for Tier B, as the verify skill already says).

## Owner decisions

- Open: sidebar (System Settings style; HIG review P2 deviation to record in
  `docs/decisions.md`) or toolbar panes (HIG default) for the Settings window.
- Decided (owner ruling 2026-10-27, this experiment's thread): accent-only
  symbol tiles "lack craft"; tiles use fixed macOS category hues
  (`settings_kit::Hue`) chosen by destination, as System Settings does. This
  overrides the HIG review's P2 on fixed-colour sidebar icons and is recorded
  as a deviation at import.

## Next iteration (from the blind review)

1. Separate focus from selection and "on": a gapped, translucent accent ring
   shared by every part (`settings_kit::ring`).
2. Promote a data-driven theme card into the kit: guaranteed foreground /
   background contrast, fixed swatch slots, one-line names, width-derived
   columns.
3. Forbid nested cards: a group holds rows; grids and tables sit outside or
   inset with clipping.
4. One sidebar table (item → glyph, hue) shared by every page; one tile size.
5. Apple-like theme parity: white selected segment in light, white switch
   knob in both themes, hairlines that do not change layout; Japanese
   line-break rules for wrapped text.
