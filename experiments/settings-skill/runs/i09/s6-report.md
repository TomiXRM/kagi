- Deliverable / evidence: [examples/lab_i09_s6.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s6.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i09_s6 --features gui-e2e`, `--list`, and offscreen host captures. PNGs: [Apple Dark EN](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s6/lab-i09-s6-default-apple-dark-en.png), [Apple Light EN](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s6/lab-i09-s6-default-apple-light-en.png), [Apple Dark JA 720×692](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s6/lab-i09-s6-default-apple-dark-ja-720x692.png).

- Requirement self-check:

  1. ○ Apple Dark and Apple Light EN host renders succeeded.
  2. ○ Standard page surfaces use `settings_kit`.
  3. ○ `theme_card(theme, selected, forced)` derives its preview from `Theme` fields, with no hex literals.
  4. ○ `themes_by_name()` is filtered by `dark`, preserving its order and rendering each matching theme once.
  5. ○ Apple Light/Dark are selected; selected names are semibold with accent rings; stable horizontal strip IDs use `overflow_x_scroll()`.
  6. ○ Commands, captures, and unobserved items are listed here. Capture critique found no [C1] or [C2] issues.

- Unclear points: none.

- Discretionary fills: Kept the fixed Themes layout. Placement: theme choice and extended palette → Themes pane → Light Mode / Dark Mode sections. Sidebar uses domain-oriented Kagi panes and category hues.

- Kit gaps: `theme_card` is the page-local theme-choice preview tile; the kit has no theme-card control.

- Retries: 0. Cargo briefly waited on the shared build-directory lock, then completed successfully.

- Not observed: Real pointer hover, keyboard focus navigation, VoiceOver, or manual horizontal scrolling; offscreen captures verified static layout only.