- Deliverable / evidence: [examples/lab_i07_s4.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s4.rs). Ran `rustfmt --edition 2021 examples/lab_i07_s4.rs`; `cargo build --example lab_i07_s4 --features gui-e2e`; `target/debug/examples/lab_i07_s4 --list`; rendered dark/light default and forced-focus states. PNGs:
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s4/lab-i07-s4-default-dark.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s4/lab-i07-s4-default-light.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s4/lab-i07-s4-focus-alert-volume-dark.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s4/lab-i07-s4-focus-alert-volume-light.png`
  - `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s4/lab-i07-s4-default-dark-ja-720x692.png`

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN host renders exited successfully.
  2. ○ Uses `settings_kit` for window, sidebar, sections, groups, rows, controls, and help capsule.
  3. ○ One `sound_slider` wrapper renders all three sliders; kit mapping keeps each thumb/fill/ticks at its value.
  4. ○ Reusable `output_device_table` has aligned headers/columns, six devices, and one theme-selected row.
  5. ○ Full-width Output/Input segmented control is selected on Output; Mute is checked and right-aligned.
  6. ○ Alert pop-up and Play capsule share the row’s trailing control cluster.
  
  Capture critique: no `[C1]` or `[C2]` findings after trimming the page height; light/dark borders, selected states, and slider indicators remain legible.

- Unclear points: `[Understanding] Speaker glyph / the permitted IconName list has no speaker variant / use an approved closest endpoint icon, or add speaker symbols centrally to the kit before requiring exact speaker glyphs.`

- Discretionary fills: Sound is the selected Pink sidebar pane; Sound Effects and Output & Input are the two sections. Placement: alert/device/startup controls → Sound → Sound Effects; output selector/devices/volume/balance → Sound → Output & Input. Used −/+ as the closest available volume-end symbols.

- Kit gaps: `output_device_table`, a page-local selectable two-column device table; the kit has no table component. No custom slider or checkbox implementation.

- Retries: 1 visual-layout retry: reduced page height from 1080 to 860 after capture review. Both builds waited for the shared Cargo file lock.

- Not observed: real pointer hover, live keyboard focus traversal, VoiceOver/accessibility behavior, or interactive pop-up/switch changes; focus is gallery-forced only.