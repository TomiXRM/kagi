# ADR-0173: Reduce-motion setting

- Status: Accepted
- Date: 2026-09-04
- Touches: `crates/kagi-ui-core/src/settings.rs`,
  `crates/kagi-ui-core/src/theme.rs`,
  `crates/kagi-ui-core/src/i18n/mod.rs`,
  `src/main.rs`, `src/ui/render_body.rs`, `src/ui/settings_view.rs`
- Issue: #354 (accessibility foundation) — **reduce-motion slice only**

## Context

Issue #354 proposes a four-stage accessibility foundation: reduce motion,
readable confirmation dialogs (a11y `role` / `on_a11y_action`), readable list
UIs, and a colorblind theme. This ADR covers **only the reduce-motion slice**.
The other three are deferred (see "Out of scope / follow-ups").

The issue asserted that GPUI's `AnimationExt` already honors `App::reduce_motion()`
automatically, so wiring an OS/setting flag through `cx.set_reduce_motion()` would
settle every animation for free. That is **not true of the GPUI revision this repo
pins** (`zed-industries/zed@90b3aa0`): `crates/gpui/src/elements/animation.rs`'s
`AnimationElement` never reads `reduce_motion`, and this build's `app.rs` has no
`reduce_motion` API at all. So an app-side gate is required — we cannot lean on
GPUI to do it.

## Decision

A flat-string setting `reduce_motion` (`"true"` | `"false"`, default **off**)
that kagi's own animations consult at render time.

- **Setting**: typed accessor `Settings::reduce_motion()` in
  `crates/kagi-ui-core/src/settings.rs` (flat string on disk per the settings
  rules; default off — only an explicit `"true"` enables it).
- **Process-global**: `theme::reduce_motion()` / `set_reduce_motion()` /
  `init_reduce_motion()`, a `static AtomicBool` mirroring the existing
  `compact_graph` / `auto_fetch` pattern in `theme.rs`. Seeded once at startup
  from `settings.json` in `main.rs` and read cheaply (one atomic load) on the
  render frames that draw a looping animation. Emits the contract line
  `[kagi] reduce_motion: <bool>` at init.
- **Loading animation** (`src/ui/render_body.rs` `render_loading_placeholder`,
  the ADR-0165 bobbing dots): when reduce-motion is on, each dot renders static
  — the `.with_animation` call is **skipped entirely** (not just zeroed) so no
  per-frame animation ticks are requested. The bob math is extracted into a pure,
  GUI-free helper `loading_dot_lift(reduce_motion, phase, delta) -> f32` that
  returns `0.0` when reduce-motion is on, unit-tested both ways
  (`loading_dot_lift_static_when_reduce_motion`,
  `loading_dot_lift_animates_when_enabled`).
- **Settings UI**: a `Switch` in the Appearance section, EN+JA strings
  (`SettingsReduceMotion` / `SettingsReduceMotionDesc`).

## OS preference

macOS `NSWorkspace.accessibilityDisplayShouldReduceMotion` (and the Linux
equivalents `gtk-enable-animations` / `prefers-reduced-motion`) are **not**
wired. GPUI in this build exposes no plumbing for it and reading it would need
per-platform FFI — out of proportion for this slice. The explicit setting is the
whole contract; if OS-preference-as-default is added later, the setting overrides
it.

## Animations honored / remaining

Honored now:

- Loading placeholder dots (`render_body.rs`) — the one animation the ticket
  scoped.

Not yet gated (all decorative loops; left as follow-ups to keep this slice
small — each would need the same `theme::reduce_motion()` check, but they render
in different modules/entities and some outside `KagiApp`):

- Sync spinner + header spinner (`src/ui/render_overlay.rs`,
  `src/ui/render_header.rs`).
- Toast enter/exit transitions (`src/ui/render_overlay.rs`) — these are one-shot,
  not loops, so lower priority.
- Commit-panel pulse (`src/ui/commit_panel_render.rs`).
- Ecosystem loader (`crates/kagi-ui-ecosystem/src/render.rs`).

## Out of scope / follow-ups (issue #354)

- **a11y `role` / `on_a11y_action` wiring** (readable confirmation dialogs, list
  UIs): a larger GPUI foundation (stable per-row IDs from commit OIDs, `Role::List`
  + item totals on `uniform_list`). Deferred.
- **Colorblind / high-contrast theme**: an aesthetic/design call (GitHub-style
  orange↔blue swap, dual-encoded graph lanes). GPUI has no high-contrast API, so
  it must be solved as a theme. Deferred.

## Verification

- Unit: `Settings::reduce_motion()` default-off / parses `"true"`
  (`reduce_motion_default_off_and_parses_true`); the pure lift helper is static
  when on and animated when off.
- **Needs a human/GUI**: confirm the loading dots actually stop bobbing when the
  toggle is on (subagents cannot exercise the GUI).
