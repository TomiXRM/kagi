- Deliverable / evidence: [examples/lab_i06_s6.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s6.rs), gallery page `lab-i06-s6`. Commands run: `rustfmt --edition 2021` and `--check` on this file; `cargo build --example lab_i06_s6`; `cargo run --example lab_i06_s6`; binary `--list`, unknown-state validation, and capture runs. Captures used the supplied RAISE/display settings, both themes, all four EN states, and narrow JA defaults. **No PNGs were produced** in `target/ui-lab/runs/i06/s6/`; every capture exited 2 with `not visible (occlusionState 8192)`.

- Requirement self-check:
  1. × Host captures failed for Apple Dark and Apple Light; build passed.
  2. ○ All available controls use `settings_kit`.
  3. partial — reusable card uses theme fields exclusively for preview colours; cross-theme appearance not captured.
  4. ○ All 15 built-ins are sourced once, filtered by mode, preserving registry order.
  5. partial — static Apple selections, emphasized names, accent rings, fixed-width cards and stable horizontal-scroll IDs implemented; visual behavior not captured.
  6. ○ Commands, capture destinations and observation limits reported.
  
  C1/C2 capture critique could not be performed because no PNG exists.

- Unclear points: `[Execution] Native window reported invisible / macOS occlusion persisted despite RAISE; display was awake and unlocked / Retry when the target display permits visible host rendering; do not bypass the visibility check.`

- Discretionary fills: 136-point cards, eight-point gaps, localized sidebar labels, and middle/end capture states. Custom themes are excluded from “Built-in.”

- Kit gaps: reusable `theme_card(theme, selected, forced)` preview choice.

- Retries: one build retry after correcting unsupported `aria_checked` to `aria_selected`; four repeated capture attempts across 14 total capture runs. Cargo lock waits were respected.

- Not observed: rendered layout, palette consistency across themes, scrolling, narrow Japanese text fit, real hover, keyboard focus, or VoiceOver. No app was activated.