- Deliverable / evidence: Created [examples/lab_i06_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i06_s2.rs).
  - Commands: `cargo build --example lab_i06_s2` passed; file-only `rustfmt` and `rustfmt --check` passed; `--list` listed seven states; unknown-state validation correctly exited 2.
  - Attempted 21 captures using the supplied display/raise settings, covering both themes, every state, and narrow EN/JA.
  - PNG paths: **none created**. All captures exited 2 with `not visible (occlusionState 8192)`. Intended destination: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i06/s2/`.

- Requirement self-check:
  1. × — Apple Dark/Light host captures failed visibility checks.
  2. ○ — Existing controls and layout use `settings_kit`.
  3. partial — Reusable preview choices, selected rings, emphasized labels and shared divided group implemented; not captured.
  4. partial — Kit text-only, equal-width segments implemented with selections; not captured.
  5. partial — Five slider ticks, trailing “3” and kit pop-ups implemented; not captured.
  6. partial — Kit geometry retained; visual alignment remains unverified.
  7. ○ — Commands, evidence limits and unobserved behavior reported.

- Unclear points: `[Execution] Native capture unavailable / Host reports window not visible / Restore a visible, nonactivating gallery capture environment before accepting visual evidence.` No capture-backed `[C1]` or `[C2]` findings could be established.

- Discretionary fills: Added Japanese labels, stylized miniature previews and seven capture states. Slider “3” represents the midpoint of five values. Interaction states preserve default selections.

- Kit gaps: Page-local `choice_tile` and its preview renderer.

- Retries: One build retry corrected `flex_grow`’s required argument; three repeated capture attempts among 21 total.

- Not observed: Actual rendered geometry, clipping, theme readability, indicator alignment, real hover, keyboard focus or VoiceOver.