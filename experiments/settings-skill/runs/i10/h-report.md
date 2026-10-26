- Deliverable / evidence: [examples/lab_i10_h.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_h.rs). Ran `rustfmt --edition 2021 examples/lab_i10_h.rs`, `cargo build --example lab_i10_h --features gui-e2e`, and `target/debug/examples/lab_i10_h --list`. Captures: [default dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-default-dark.png), [default light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-default-light.png), [focus-new dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-focus-new-dark.png), [focus-new light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-focus-new-light.png), [focus-import dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-focus-import-dark.png), [focus-import light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/h/lab-i10-h-focus-import-light.png).

- Requirement self-check:
  - ○ Apple Dark and Light EN host renders succeeded.
  - ○ Standard controls are settings-kit components.
  - ○ “New” uses `section_action` as the section accessory.
  - ○ Reusable swatch cards have a selected ring; captions are separated with the kit divider.
  - ○ Import uses kit `Row` and `capsule_button`.
  - ○ Commands, captures, and unobserved evidence are listed. Capture critique: `[C1] none`; `[C2] none`.

- Unclear points: none

- Discretionary fills: Four visible cards per mode keep every card and row fully visible at the 900×760 logical gallery size; the supplied layout remains one Themes pane.

- Kit gaps: `theme_swatch` and its two-mode `theme_choices` composition, needed because the kit has no visual theme-preview selector.

- Retries: 0. Cargo waited for another build’s file lock, then completed successfully.

- Not observed: real pointer hover, keyboard navigation/focus, VoiceOver, localization beyond EN, or live import/theme-selection actions.