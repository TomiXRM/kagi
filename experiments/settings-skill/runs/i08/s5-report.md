- Deliverable / evidence: [examples/lab_i08_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s5.rs). Ran `rustfmt --edition 2021 examples/lab_i08_s5.rs`, `cargo build --example lab_i08_s5 --features gui-e2e`, `target/debug/examples/lab_i08_s5 --list`, gallery captures, and `git diff --check`. PNGs: [dark default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s5/lab-i08-s5-default-apple-dark.png), [light default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s5/lab-i08-s5-default-apple-light.png), hover-card and focus-help captures for both themes, plus [JA / 720×692](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s5/lab-i08-s5-default-apple-dark-ja-720.png).

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN host renders succeeded.
  2. ○ Uses settings-kit window, sidebar, section, group, row, tile, capsule, progress, and navigation parts.
  3. ○ Reusable `icloud_card(symbol, tint, title, subtitle, forced)`; both grids are 3×2 with equal-height cards.
  4. ○ Reusable `storage_progress`; `Manage…` aligns with section and navigation trailing controls.
  5. ○ Values render before kit navigation chevrons.
  6. ○ Saved section has a green status dot and `See All`.
  7. ○ Commands, evidence paths, and limitations listed here. Capture critique fixed two `[C1]` findings: card boundaries were too subtle, and the lower rows were too near the viewport edge.

- Unclear points: `[Planning] Local Apple HIG corpus unavailable / the mandated corpus path was absent and Apple’s Markdown endpoint was not fetchable here / keep independent HIG review pending before merge.`

- Discretionary fills: compact representative sidebar; blue globe account badge; forced `hover-card` and `focus-help` gallery states.

- Kit gaps: profile header/badge, reusable grid card, leading section-status slot workaround, and corner help button.

- Retries: 3 — two visual recapture corrections; one invalid-state check reissued after zsh rejected the reserved `status` variable.

- Not observed: real pointer hover, keyboard focus behavior, VoiceOver, independent HIG review, or localized Japanese copy (the scenario content is English).