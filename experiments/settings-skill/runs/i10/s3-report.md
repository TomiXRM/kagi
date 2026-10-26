- Deliverable / evidence: [examples/lab_i10_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s3.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i10_s3 --features gui-e2e`, gallery listing, four offscreen renders, and an invalid-state check. PNGs: `target/ui-lab/runs/i10/s3/lab-i10-s3-default-en-dark.png`, `lab-i10-s3-default-ja-light.png`, `lab-i10-s3-default-ja-dark-720x692.png`, `lab-i10-s3-focus-auto-fetch-en-dark.png`.

- Requirement self-check:
  - ○ Host render succeeds for EN dark, JA light, and 720×692 JA.
  - ○ All UI parts are from `settings_kit`.
  - ○ Titles and descriptions use `Msg`; static values are only “Apple Dark”, “100%”, and “70rem”.
  - ○ Japanese wraps cleanly at 720 pt; the lower Network content is below the normal scroll viewport, not overlapping or horizontally clipped.
  - ○ Sidebar has four Kagi panes, category-hued icon tiles, and General as the sole selected item.
  - ○ Commands, captures, and unobserved behavior are listed here.

- Unclear points: none

- Discretionary fills: Appearance / Graph / Network placement follows the supplied General layout. Used Gray, Blue, Purple, and Teal sidebar hues; added a forced `focus-auto-fetch` capture state without changing values.

- Kit gaps: none

- Retries: 0; Cargo briefly waited for another build’s file lock, then completed successfully.

- Not observed: real pointer hover, keyboard focus navigation, scrolling interaction, and VoiceOver.