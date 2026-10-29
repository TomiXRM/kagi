- Deliverable / evidence: Created only [examples/lab_i06_s4.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s4.rs), page `lab-i06-s4`.
  - Commands: `rustfmt --edition 2021 examples/lab_i06_s4.rs`; `cargo build --example lab_i06_s4` (twice, passed); single-file rustfmt check (passed); binary `--list`; unknown-state invocation (correctly exited 2).
  - Attempted both themes across all five states, plus EN/JA at `720x692`, using the specified display and raise settings.
  - PNG paths: **none produced**. All 16 capture attempts exited 2 with `not visible (occlusionState 8192)`. Intended destination: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s4/`.

- Requirement self-check:
  1. **partial** — Build succeeds; both theme captures were refused, so host rendering is not captured.
  2. **○** — All listed control kinds use `settings_kit`; local components cover missing table and speaker symbols.
  3. **partial** — One reusable function composes all three kit sliders. Endpoint states implemented; thumb positions not captured.
  4. **partial** — Reusable table has aligned columns, six devices, and one selected row using `ink::selected()`; appearance unverified.
  5. **partial** — Output selected and Mute checked/aligned in code. **[C1]** Segmented control remains intrinsic-width: the kit exposes no full-width cell API.
  6. **partial** — Pop-up and play button share a trailing cluster; visual alignment not captured.
  7. **○** — Commands, evidence failure, and observation limits reported.

- Unclear points:
  - `[Planning]` Full-width segmented requirement conflicts with available kit API / Cells have fixed intrinsic widths / Add responsive width support to the kit owner.
  - `[Execution]` Native capture unavailable / Host reports invisible windows despite the specified settings / Restore window visibility and rerun captures without activation.

- Discretionary fills: `900x900` default size; slider values `0.94`, `0.04`, `0.5`; seven alert ticks; nearest available sidebar icons; EN/JA text; forced focus, hover, and endpoint states.

- Kit gaps: Page-local reusable device table and speaker symbols. Full-width segmented capability remains unresolved.

- Retries: Two repeated capture configurations; 16 total attempts. Cargo lock waits were allowed to finish.

- Not observed: No PNG critique was possible. Theme contrast, clipping, indicator placement, trailing alignment, real hover, keyboard focus, and VoiceOver remain unverified. No app was activated.