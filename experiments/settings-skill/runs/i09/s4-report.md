- Deliverable / evidence: [examples/lab_i09_s4.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s4.rs). Commands run: `rustfmt --edition 2021 examples/lab_i09_s4.rs`; `cargo build --example lab_i09_s4 --features gui-e2e`; gallery `--list`; six offscreen captures; invalid-state check. PNGs: [dark default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s4/lab-i09-s4-default-apple-dark.png), [light default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s4/lab-i09-s4-default-apple-light.png), plus focus/hover captures for both themes in the same directory.

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN host renders succeeded.
  2. ○ Standard controls are from `settings_kit`; only the device table is page-local.
  3. ○ One `sound_slider` wrapper renders alert, output, and balance sliders at 0.92, 0.08, and 0.5.
  4. ○ Reusable device table has aligned Name/Type columns and one theme-selected row.
  5. ○ Full-width Output/Input segmented control is visibly selected; checked Mute aligns under the output slider.
  6. ○ Alert pop-up and Play capsule share the row’s trailing control cluster.
  - Capture critique: no `[C1]` or `[C2]` findings after reviewing dark/light default, focus, and hover captures.

- Unclear points:
  - [Understanding] Speaker glyph unavailable / the approved `IconName` list has no speaker icon / use the nearest provided symbol rather than inventing an icon.
  - [Planning] Independent HIG corpus unavailable / the configured local Apple-design corpus is absent / install the designated corpus before production UI/PR work.

- Discretionary fills: Apple-category sidebar uses required category hues; `Bell` is the closest permitted sound symbol; the play action is a labelled kit capsule.

- Kit gaps: `device_table` / `device_table_row` only; the kit has no table component.

- Retries: 2 — corrected unsupported `aria_selected` and converted borrowed table labels to owned strings.

- Not observed: real pointer hover/focus, keyboard interaction, VoiceOver/accessibility-tree behavior, Japanese layout.