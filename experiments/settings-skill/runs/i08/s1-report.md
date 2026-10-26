- Deliverable / evidence: [examples/lab_i08_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s1.rs). Ran `rustfmt --edition 2021 examples/lab_i08_s1.rs`, `cargo build --example lab_i08_s1 --features gui-e2e`, gallery `--list`, and dark/light EN captures for all states. PNGs: `target/ui-lab/runs/i08/s1/lab-i08-s1-{default,rest-picker,selected-top-left,focus-picker,hover-picker}-{apple-dark,apple-light}.png`; narrow JA-environment capture: `lab-i08-s1-default-ja-720x692-apple-dark.png`.

- Requirement self-check:
  1. ○ Apple Dark and Apple Light EN host renders succeeded.
  2. ○ Uses `settings_kit` for scaffold, sidebar, sections, groups, rows, switches, navigation rows, and tiles.
  3. ○ Required order/content, subtitle, and final off switch match the scenario.
  4. ○ `position_picker(selected, forced)` is parameterized; states include rest, selected-value, and focus.
  5. ○ Capture review confirms kit row geometry and common trailing edge.
  6. ○ Default plus focus and hover forced captures visibly differ.
  7. ○ Commands, paths, and limitations are listed here.

- Unclear points: none

- Discretionary fills: used an 820pt page height to expose every required row; used kit status tints for Working/Blocked/Error/Done and category hues in the sidebar.

- Kit gaps: `position_picker`, a page-local reusable 3×2 notification-placement grid because the kit has no grid picker.

- Retries: 3 — corrected the picker return type after compilation, increased unselected-cell contrast after dark-capture review, and moved the forced-hover state to the picker so it renders visibly.

- Not observed: real pointer hover, keyboard focus traversal, VoiceOver/accessibility interaction, or localized Japanese copy; captures use forced gallery states and the page’s specified English content.