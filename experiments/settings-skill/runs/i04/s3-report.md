- Deliverable / evidence: [examples/lab_i04_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i04_s3.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i04_s3`, `--list`, five native gallery captures, and an invalid-state check (exit 2, listing valid states). PNGs: `target/ui-lab/runs/i04/s3/lab-i04-s3-{default,focus-auto-fetch}-en-apple-dark.png`, `lab-i04-s3-{default,focus-auto-fetch}-ja-apple-light.png`, and `lab-i04-s3-default-ja-apple-light-720x692.png`.

- Requirement self-check:
  - ○ Host render succeeded for Apple Dark EN, Apple Light JA, and 720×692 JA.
  - ○ Uses only `settings_kit` page, section, group, row, switch, segmented, popup, stepper, tile, and sidebar components.
  - ○ Visible titles and descriptions use `Msg`.
  - ○ Narrow JA capture wraps long descriptions without overlap or trailing-control escape.
  - ○ Sidebar shows General, Language, Smart Commit, and Analyze; only General is selected.
  - ○ Commands, PNG paths, and unobserved behavior are listed here. `[C1]` none; `[C2]` none.

- Unclear points: none

- Discretionary fills: Added a `focus-auto-fetch` forced-state capture; retained the reference’s selected `Icon Only` and `Theme accent` values.

- Kit gaps: none

- Retries: 1 — added the missing `section` import after the initial build error.

- Not observed: real pointer hover, keyboard interaction, VoiceOver, or persistence behavior; this static gallery page intentionally writes no settings.