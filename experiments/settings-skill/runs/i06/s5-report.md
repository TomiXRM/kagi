- Deliverable / evidence: [examples/lab_i06_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s5.rs). Ran `rustfmt --edition 2021 examples/lab_i06_s5.rs`, `cargo build --example lab_i06_s5`, and `target/debug/examples/lab_i06_s5 --list`. Unknown state correctly exits with valid-state names. Attempted PNG: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s5/lab-i06-s5-default-dark.png` — not written; the host rejected the window as occluded.

- Requirement self-check:
  1. × Apple Dark/Light host capture — blocked by `occlusionState 8192`; no PNG was produced.
  2. ○ Kit reuse — standard settings controls use `settings_kit`; documented page-local gaps only.
  3. ○ Reusable `cloud_card` with six equal grid cells, fixed shared height, and two-line title/subtitle protection.
  4. ○ Reusable `storage_meter` wraps the Kit progress bar; `Manage…` is a trailing capsule control.
  5. ○ Navigation rows render their trailing values before Kit navigation chevrons.
  6. ○ “Saved to iCloud” has a leading green status dot and trailing `See All`.
  7. ○ This report lists commands, attempted PNG path, and unobserved evidence.

- Unclear points: none

- Discretionary fills: Used the allowed `Globe` symbol for the unavailable cloud badge/icon; chose a 920×900 gallery canvas to fit both grids. Independent HIG review found no remaining P1/P2; the Help button has explicit hover/pressed/disabled states. Its lower-trailing placement and `Manage…` ellipsis follow Apple’s [Buttons HIG](https://developer.apple.com/design/human-interface-guidelines/buttons).

- Kit gaps: page-local three-column cloud cards/grid; account hero header; status-decorated section heading because `section` has no leading accessory; circular Help button because the Kit has no Help control.

- Retries: 4 capture attempts, each rejected because the specified display reported the gallery window as not visible. Two compile corrections were also made for an unavailable icon variant and a `role` call requiring an element ID.

- Not observed: native dark/light PNG appearance, real hover/focus, keyboard activation, VoiceOver, and capture-based C1/C2 visual critique.