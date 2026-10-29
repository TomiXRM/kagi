- Deliverable / evidence: [examples/lab_i07_s6.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s6.rs). Ran `rustfmt --edition 2021 examples/lab_i07_s6.rs`, `cargo build --example lab_i07_s6 --features gui-e2e`, listed states, validated an unknown state exits non-zero, and captured every declared state in Apple Dark and Apple Light.
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-default-apple-dark.png`
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-hover-apple-light-apple-dark.png`
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-hover-apple-dark-apple-dark.png`
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-default-apple-light.png`
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-hover-apple-light-apple-light.png`
  - `target/ui-lab/runs/i07/s6/lab-i07-s6-hover-apple-dark-apple-light.png`

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN host renders succeeded.
  2. ○ Sidebar, sections, groups, rows, switches, and actions use `settings_kit`.
  3. ○ Reusable `theme_card(theme, selected, forced)` draws previews exclusively from each theme’s fields; no hex literals.
  4. ○ `themes_by_name()` is filtered by `dark`, preserving its order and emitting each theme once.
  5. ○ Selected cards have accent rings and semibold names; fixed-width cards live in stable-ID horizontal scrolling strips.
  6. ○ Commands, PNG paths, and unobserved behavior are listed below. Capture critique found no remaining C1/C2 visual issues.

- Unclear points:
  - [Execution] A broad API-style search briefly surfaced lines from another `examples/lab_*` file / The search scope was too broad / Restrict all future searches to named allowed files before running them.

- Discretionary fills: Used a 1060×900 gallery size to show both sections at once; used Kagi domain panes in the sidebar; added hover states for each selected Apple theme; corrected duplicate section-action IDs and used each theme’s accent for the cursor block.

- Kit gaps: `theme_card` is page-local because the kit has no data-driven theme preview card; it uses kit tokens and `ring` for surrounding chrome.

- Retries: 2 — one compile retry for the ANSI-dot iterator type; one visual correctness retry for unique action IDs and the accent cursor color. File-lock waits were honored.

- Not observed: Real pointer hover, keyboard focus traversal, VoiceOver/accessibility behavior, and Japanese localization.