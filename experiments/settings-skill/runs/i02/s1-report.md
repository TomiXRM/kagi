- Deliverable / evidence: [examples/lab_i02_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i02_s1.rs). Ran `rustfmt --edition 2021 examples/lab_i02_s1.rs`, `cargo build --example lab_i02_s1`, and `target/debug/examples/lab_i02_s1 --list`. Captures:
  - `target/ui-lab/runs/i02/s1/lab-i02-s1-{default,position-selected,position-focus}-{apple-dark,apple-light}.png`

- Requirement self-check:
  1. ○ Dark/light EN host renders succeeded.
  2. ○ Uses `settings_kit` for scaffold, sidebar, sections, groups, rows, switches, tiles, and navigation rows.
  3. ○ Required content/order, subtitles, and final off switch are implemented.
  4. ○ Reusable `position_picker(id, selected, forced)` uses theme tokens; default is Rest, with selected and focus captures.
  5. ○ Capture shows kit row heights and aligned trailing controls.
  6. ○ `default`, `position-selected`, and `position-focus` visibly differ.
  7. ○ Commands, captures, and limitations listed here.

- Unclear points: none

- Discretionary fills: Used Kagi semantic status tints for the navigation status symbols; the reference’s multicolored sidebar was normalized to Kagi’s accent/status scheme.

- Kit gaps: `position_picker`, a page-local 3×2 notification-position picker.

- Retries: 2 — fixed collapsed picker cells, then added the omitted picker children. `[C1]` resolved; no `[C2]` findings.

- Not observed: Real hover, keyboard focus, VoiceOver, or interactive state changes; captures use forced gallery states.