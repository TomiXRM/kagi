- Deliverable / evidence: [examples/lab_i06_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s3.rs). Ran `rustfmt --edition 2021 examples/lab_i06_s3.rs`, `cargo build --example lab_i06_s3`, `target/debug/examples/lab_i06_s3 --list`, captures with the specified display/raise settings, and verified unknown-state rejection. PNGs:
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-default-en-dark.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-focus-auto-fetch-en-dark.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-default-ja-light.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-focus-auto-fetch-ja-light.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-default-ja-dark-720.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s3/lab-i06-s3-focus-auto-fetch-ja-dark-720.png`

- Requirement self-check:
  - ○ Host render: Apple Dark EN, Apple Light JA, and 720 pt JA all wrote native PNGs successfully.
  - ○ Kit reuse: all page structure and controls use `settings_kit`; no page-local control was added.
  - ○ Localization: visible labels and descriptions use `Msg`; only requested static values (`Apple Dark`, `100%`, `70rem`) are literals.
  - ○ JA/narrow layout: long descriptions wrap inside their rows; trailing controls remain aligned and inside groups. The narrow viewport scrolls for lower content.
  - ○ Sidebar: General, Language, Smart Commit, and Analyze use distinct category-hue icon tiles; exactly General is selected.
  - Capture critique: `[C1] none`; `[C2] none`.

- Unclear points: none

- Discretionary fills: added the deterministic `focus-auto-fetch` gallery state; used `SettingsTitle` as the localized sidebar search placeholder.

- Kit gaps: none

- Retries: 1 — the unknown-state verification initially used zsh’s reserved `status` variable; retried with `gallery_status`.

- Not observed: real pointer hover, keyboard-driven focus, VoiceOver/AX behavior, and an independent HIG review.