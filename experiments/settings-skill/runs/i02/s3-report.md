- Deliverable / evidence: [examples/lab_i02_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i02_s3.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i02_s3`, and `target/debug/examples/lab_i02_s3 --list`; rendered six native captures and verified an unknown state exits non-zero. PNGs: `target/ui-lab/runs/i02/s3/lab-i02-s3-*.png`.

- Requirement self-check: ○ Host renders: Apple Dark EN, Apple Light JA, and 720×692 JA all exited 0. ○ Kit reuse: only `settings_kit` page composition used. ○ Localization: visible titles/descriptions use `Msg`; static values are Apple Dark, 100%, and 70rem. ○ Narrow JA: long descriptions wrap with controls retained in-card. ○ Sidebar: four icon-tile panes, exactly General selected. ○ Report includes commands, paths, and limitations.

- Unclear points: none

- Discretionary fills: Added `focus-ui-zoom` as the distinct forced-state capture; used Accent sidebar tiles per HIG. Capture critique: `[C1] none`; `[C2] none`.

- Kit gaps: none

- Retries: 1 — the first unknown-state check included `--list`, which exits before validating the state; reran without it and confirmed the expected non-zero exit.

- Not observed: real pointer hover, actual keyboard focus traversal, VoiceOver, or interaction callbacks; focus is forced for the gallery capture.