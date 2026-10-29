- Deliverable / evidence: [examples/lab_i03_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i03_s2.rs). Ran `rustfmt --edition 2021 examples/lab_i03_s2.rs`, `cargo build --example lab_i03_s2`, `target/debug/examples/lab_i03_s2 --list`, six EN Apple Dark/Light gallery captures on display `ad1ab626`, and invalid-state validation (exit 2).
  
  PNGs: `lab-i03-s2-{default,focus-slider,pressed-choice}-{dark,light}.png` under `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i03/s2/`.

- Requirement self-check:
  1. ○ Host renders in Apple Dark and Apple Light (EN); all captures written successfully.
  2. ○ Standard settings elements are composed from `settings_kit`.
  3. ○ Reusable `choice_tile(preview, label, selected, forced)` with accent selection ring/emphasized label; clusters share one group with `vertical_divider()`.
  4. ○ Kit text-only equal-width segmented controls show the requested selections.
  5. ○ Five-tick slider shows value `3`; pop-up rows use `popup_value`.
  6. ○ Capture review confirms aligned trailing controls, kit group geometry, visible borders in both themes. [C1] none; [C2] none.
  7. ○ Commands, PNGs, and unobserved behavior are recorded here.

- Unclear points: none

- Discretionary fills: Used theme-derived preview glyphs instead of reproducing Rex artwork; nonselected sidebar tiles are neutral, except the “Server” running-status tile, while selected Interface is accent as required.

- Kit gaps: `choice_tile` visual-preview selector; `ticked_slider` five-position slider.

- Retries: 1 — the invalid-state validation wrapper initially used zsh’s readonly `status`; reran it with `result_code`. No source or capture retry.

- Not observed: real pointer hover, keyboard focus/input, VoiceOver/AX behavior, and Japanese localization.