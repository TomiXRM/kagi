- Deliverable / evidence: [examples/lab_i09_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s3.rs), gallery page `lab-i09-s3`. Only this source file was written.

  Commands run: `rustfmt --edition 2021 examples/lab_i09_s3.rs`, `cargo build --example lab_i09_s3 --features gui-e2e`, binary `--list`, three gallery captures, invalid-state rejection, and file-only `rustfmt --check`. Build and captures passed; invalid state correctly exited 2.

  PNGs, all opened and inspected:
  [Dark EN](target/ui-lab/runs/i09/s3/default-en-dark.png) · [Light JA](target/ui-lab/runs/i09/s3/default-ja-light.png) · [720 pt Light JA](target/ui-lab/runs/i09/s3/narrow-ja-light.png).

- Requirement self-check:

  1. ○ All three required host renders succeeded.
  2. ○ Every required component comes from `settings_kit`; geometry remains kit-owned.
  3. ○ All titles and descriptions use `Msg`. “Apple Dark” is the static theme value.
  4. ○ Japanese descriptions wrap without clipping or overlap at 720×900 pt; every requested row is captured.
  5. ○ Four sidebar panes have distinct category-coloured tiles; only General is selected.
  6. ○ Commands, evidence paths, and unobserved behavior are reported.

  Capture critique: no unresolved `[C1]` or `[C2]` findings. Indicators match their values, controls align, and both themes remain readable. Sampled segmented tracks and stepper buttons measure 48 physical pixels tall at 2×.

- Unclear points: none.

- Discretionary fills: Used 900×900 pt normally and 720×900 pt narrowly to capture every row. Preserved the reference’s Theme accent and Icon Only selections.

  Placement suggestions—the requested layout was retained:

  | Settings | Current pane → section | Future pane |
  |---|---|---|
  | Theme, Accent, Toolbar, Zoom, threshold | General → Appearance | Appearance |
  | Compact graph, Avatar nodes | General → Graph | Graph |
  | Auto-fetch | General → Network | Network & Sync |

- Kit gaps: none; no page-local controls.

- Retries: 0 implementation or capture retries. Waited for Cargo’s shared build lock.

- Not observed: real hover, keyboard focus, VoiceOver, or interactive navigation. Captures rendered offscreen; no app was activated.