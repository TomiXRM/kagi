- **Deliverable / evidence:** [examples/lab_i01_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i01_s3.rs), gallery page `lab-i01-s3`, state `default`.

  Commands run:
  - `rustfmt --edition 2021 examples/lab_i01_s3.rs`
  - `rustfmt --edition 2021 --check examples/lab_i01_s3.rs`
  - `cargo build --example lab_i01_s3`
  - `target/debug/examples/lab_i01_s3 --list`
  - `KAGI_GALLERY_STATE=unknown target/debug/examples/lab_i01_s3` — exited 2 and listed `default`.
  - Capture commands used `KAGI_GALLERY_DISPLAY=ad1ab626`, theme, language, state, output path, and `KAGI_GALLERY_SIZE=720x692` for narrow views. The three required configurations passed through `cargo run --example lab_i01_s3`; additional captures used the built binary.

  PNGs, all opened and inspected:
  - [Dark EN](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-dark-en.png)
  - [Light EN](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-light-en.png)
  - [Dark JA](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-dark-ja.png)
  - [Light JA](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-light-ja.png)
  - [Dark JA narrow](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-dark-ja-narrow.png)
  - [Light JA narrow](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s3/lab-i01-s3-default-light-ja-narrow.png)

- **Requirement self-check:**
  1. ○ Required native captures wrote PNGs and exited 0.
  2. ○ Every control, row, group, section, and scaffold uses `settings_kit`; no custom geometry.
  3. ○ All titles, descriptions, and segmented labels use `Msg`. Static values are `Apple Dark`, `100%`, and `70rem`.
  4. ○ JA descriptions wrap without overlap or clipping at both widths; trailing controls remain contained.
  5. ○ Four panes have accent icon tiles; General alone is selected.
  6. ○ Commands, PNG paths, and observation limits are reported.

- **Unclear points:** none.

- **Discretionary fills:** Selected Theme accent and Icon Only to match the reference; used Settings2, Globe, Bot, and ChartPie sidebar icons. Kept one default state and static values with no setting writes.

- **Kit gaps:** none; no page-local controls.

- **Retries:** 0 build/capture failure retries; 1 tool-connection retry. Cargo’s shared lock was awaited.

- **Not observed:** Real hover, keyboard focus/navigation, VoiceOver, or interactive value changes. No foreground activation was performed.