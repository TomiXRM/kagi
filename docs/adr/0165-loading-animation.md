# ADR-0165: Animated loading placeholder for the initial tab load

- Status: Accepted
- Date: 2026-09-04

## Context

Opening a large repository (uncached first open) takes seconds; during the
W6-TABSPEED background load the center pane shows a static "Loading <name>…"
label plus a non-animated ⟳ glyph (`render_loading_placeholder`,
`src/ui/render_helpers.rs`). The gate is `KagiApp.loading_tab: Option<…>` —
set in `tabs.rs` when an uncached tab starts loading, cleared when the
snapshot applies. There is no motion, so a 3-second load reads as a hang.

## Decision

Upgrade the existing placeholder in place — no change to load logic, timing,
state, or the `loading_tab` gate:

- Replace the static ⟳ with a **mini commit graph**: three dots colored
  `color_branch` / `accent` / `color_success` that bob in a gentle staggered
  wave, reading as commit nodes.
- Reuse the established GPUI animation idiom (`AnimationExt::with_animation`
  + `Animation::new(..).repeat()`), the same one used by the sync spinner
  (`render_overlay.rs`) and the ecosystem loader
  (`kagi-ui-ecosystem/src/render.rs`). No new dependencies or mechanisms.
- Motion is deliberately subtle (5 px amplitude at 1x zoom, 1.4 s cycle,
  positive-half sine so dots rest between hops). There is no reduce-motion
  setting in kagi today; gentle-by-default is the mitigation. If one is
  added later, this placeholder should honor it.
- The label keeps the existing localized `i18n::loading_fmt` string (EN+JA),
  so no new `Msg` entries are needed.

## Constraints observed

- `with_animation` does not tick inside an `overflow_y_scroll` container
  (documented in the ecosystem loader); the placeholder is not scrolled.
- Purely additive UI feedback in `src/ui/` — no git access, no `[kagi]`
  contract lines touched, no headless-visible behavior change.

## Consequences

- Uncached tab opens now show visible, on-theme motion while the background
  load runs; the indicator disappears with the placeholder when
  `loading_tab` clears.
- GUI-only change: verified by human eyeball (headless harness does not
  cover animation frames).
