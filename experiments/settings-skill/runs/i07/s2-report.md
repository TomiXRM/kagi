- Deliverable / evidence: [examples/lab_i07_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s2.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i07_s2 --features gui-e2e`, `--list`, and gallery captures. PNGs: `target/ui-lab/runs/i07/s2/lab-i07-s2-{default,focus-choice,hover-popup}-{dark,light}.png`, plus 720pt default light/dark captures.

- Requirement self-check:
  1. ○ Apple Dark/Light EN host renders succeeded.
  2. ○ Standard controls use `settings_kit`.
  3. ○ Reusable `choice_tile(preview, label, selected, forced)` with accent ring/emphasized label; clusters share one group and divider.
  4. ○ Kit text-only segmented controls show selections.
  5. ○ Five-tick slider, trailing `3`, and kit pop-up values present.
  6. ○ Captures show aligned trailing controls, consistent rows/groups, and no clipping at 900pt or 720pt.
  7. ○ Commands, PNGs, and unobserved items listed here. `[C1] none. [C2]` focus was initially indistinct from selection; added forced-focus elevation and recaptured.

- Unclear points: none

- Discretionary fills: Used compact blue visual previews and category-hued sidebar icons; included Rex-like sidebar destinations beyond the selected Interface item.

- Kit gaps: `choice_tile` is page-local because the kit has no visual-choice tile.

- Retries: 2 — a concurrent non-`gui-e2e` build temporarily replaced the example binary, causing an occlusion capture error; rebuilt with `gui-e2e` and recaptured.

- Not observed: Real pointer hover, keyboard focus, VoiceOver, and Japanese localization behavior; captures are offscreen gallery renders.