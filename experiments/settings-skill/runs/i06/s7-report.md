- Deliverable / evidence: [examples/lab_i06_s7.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s7.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i06_s7`, `rustfmt --check`, `git diff --check`, and `target/debug/examples/lab_i06_s7 --list`. Capture attempts used the required display and raise settings, but macOS returned `window … is not visible (occlusionState 8192)` each time, so no PNG was written. Intended paths: `target/ui-lab/runs/i06/s7/lab-i06-s7-{default,focus-name,focus-swatch}-{apple-dark,apple-light}.png`.

- Requirement self-check:
  1. partial — builds successfully; Apple Dark/Light native host captures could not run because the requested display was not visible.
  2. ○ — standard sidebar, detail page, sections, groups, rows, switch, icon tiles, and capsule controls use `settings_kit`.
  3. partial — `swatch(color, label, forced)` is reusable; its hex text derives from the RGB tuple sourced from active `term_*` values. Visual column alignment was not captured.
  4. partial — reusable `name_field` has named `focus-name` forced focus state; its rendered appearance was not captured.
  5. ○ — footer has one primary `Save & Apply`, with `Cancel` immediately before it and separated leading `Revert…`.
  6. partial — source composes a dimmed settings page, overlay, rounded elevated sheet, and shadow; native visual proof was unavailable.
  7. partial — commands and intended PNG paths are listed; no captures exist due to the visibility refusal.

  `[C1]` No source-level skill/HIG violation found. `[C2]` Native light/dark visual critique remains unavailable without captures.

- Unclear points: none

- Discretionary fills: used the active Kagi theme’s terminal palette for all swatches; used six Terminal columns and eight ANSI columns to match the reference’s compact palette layout; supplied `focus-swatch` alongside the required focused-name state.

- Kit gaps: reusable palette swatch, editable-looking name field, sheet container/scrim, full-width palette action, and sheet footer buttons (including the single primary action).

- Retries: 7 — four syntax/API correction-and-rebuild cycles; three gallery capture attempts refused because the specified display was not visible. One build waited for Cargo’s concurrent file lock as instructed.

- Not observed: real hover, keyboard focus, VoiceOver, live interaction, and all required native PNG captures in Apple Dark and Apple Light.