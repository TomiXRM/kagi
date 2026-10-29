- Deliverable / evidence: [examples/lab_i10_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s1.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i10_s1 --features gui-e2e`, `target/debug/examples/lab_i10_s1 --list`, and offscreen gallery renders. PNGs: `target/ui-lab/runs/i10/s1/lab-i10-s1-{default,rest-picker,selected-bottom-left,focus-picker,focus-switch,hover-nav}-apple-{dark,light}.png`, plus `lab-i10-s1-default-apple-dark-ja.png` and `lab-i10-s1-default-apple-light-720x692.png`. Independent HIG re-review: no P1/P2 findings.

- Requirement self-check:
  1. ○ Apple Dark/Light EN host renders succeeded.
  2. ○ Kit primitives used for scaffold, sidebar, sections, groups, rows, switches, navigation, and tiles.
  3. ○ Required ordering/content, subtitle, and final off switch match.
  4. ○ `position_picker(selected, forced)` is page-local, theme-token-only, and has rest/selected/focus states.
  5. ○ Final captures show aligned trailing controls and kit-owned row geometry.
  6. ○ `focus-picker`, `focus-switch`, and `hover-nav` visibly differ from default.
  7. ○ Commands, capture paths, and limitations are listed here.

- Unclear points: none

- Discretionary fills: used semantic sidebar hues; status symbols are loader/cross/alert/check. The selected-value capture changes both picker check and subtitle to bottom-left.

- Kit gaps: 3×2 notification position picker only.

- Retries: 3 — replaced non-visible forced hover with switch focus; regenerated one interrupted zero-byte capture; addressed two HIG P2s (picker alignment and navigation highlight).

- Not observed: real pointer hover, native keyboard focus traversal, VoiceOver, or foreground-window behavior; captures use forced offscreen states.