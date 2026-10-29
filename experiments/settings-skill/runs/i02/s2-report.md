- Deliverable / evidence: Created only [examples/lab_i02_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i02_s2.rs), gallery page `lab-i02-s2`.

  Commands run:
  - `rustfmt --edition 2021 examples/lab_i02_s2.rs`
  - `rustfmt --edition 2021 --check examples/lab_i02_s2.rs`
  - `cargo build --example lab_i02_s2`
  - `target/debug/examples/lab_i02_s2 --list`
  - Gallery capture commands using `KAGI_GALLERY_DISPLAY=ad1ab626`, both themes, all three states, and EN/JA width checks.
  - Unknown-state check: exited **2** and listed valid states.

  All captures exited **0**. PNGs are under [target/ui-lab/runs/i02/s2/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i02/s2):
  - `lab-i02-s2-default-dark.png`
  - `lab-i02-s2-default-light.png`
  - `lab-i02-s2-focus-choice-apple-dark.png`
  - `lab-i02-s2-focus-choice-apple-light.png`
  - `lab-i02-s2-focus-slider-apple-dark.png`
  - `lab-i02-s2-focus-slider-apple-light.png`
  - `lab-i02-s2-default-apple-dark-en-720.png`
  - `lab-i02-s2-default-apple-dark-ja-720.png`
  - `lab-i02-s2-default-apple-light-en-720.png`
  - `lab-i02-s2-default-apple-light-ja-720.png`

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN native captures succeeded.
  2. ○ All existing control kinds come from `settings_kit`.
  3. ○ Reusable choice function accepts preview, label, selection and forced state; selected labels are emphasized and ringed. Both clusters share one group and kit divider.
  4. ○ Kit segmented controls show equal-width, text-only segments and their selections.
  5. ○ Slider has five ticks, centered value `3`, and trailing value; both pop-ups use the kit.
  6. ○ Kit rows, hairlines, group radii and trailing alignment retained. Indicators match their values; controls remain readable in both themes. **[C1] fixed:** Light/Dark preview colors initially reversed meaning in the light theme. No remaining C1/C2 findings.
  7. ○ Commands, PNG paths and observation limits reported.

- Unclear points:
  - `[Understanding]` General segmented-choice rule versus explicit preview-tile scenario / overlapping instructions / follow the scenario’s specific tile requirement; use kit segments for Tab and Pane style.
  - `[Execution]` Inline image previews appeared to omit artwork / saved PNG pixel checks showed it present / inspect original file data before diagnosing a rendering failure.

- Discretionary fills: Schematic preview artwork uses kit colors; slider range is 1–5; added Japanese labels and forced-focus states. Sidebar tiles use Accent, following §3. Japanese sidebar text uses the kit’s standard truncation.

- Kit gaps: `choice_tile`, its decorative preview artwork, and `ticked_slider`.

- Retries: Seven implementation revision rounds: accessibility IDs, theme correction, three thumbnail-layout attempts, canvas artwork, and import cleanup. One patch-context retry and one capture-only retry round.

- Not observed: Real hover, keyboard focus/navigation, VoiceOver, live value changes, or persistence. Focus appearances were forced. No app was activated or brought to the foreground.