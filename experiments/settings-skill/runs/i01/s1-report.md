- Deliverable / evidence: [examples/lab_i01_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i01_s1.rs), gallery page `lab-i01-s1`. Only this source file was added.

  Commands run:
  - `rustfmt --edition 2021 examples/lab_i01_s1.rs`
  - `rustfmt --edition 2021 --check examples/lab_i01_s1.rs`
  - `cargo build --example lab_i01_s1`
  - `target/debug/examples/lab_i01_s1 --list`
  - `cargo run --example lab_i01_s1` with gallery capture environment variables
  - Direct binary captures for every state in both themes, plus JA at both sizes
  - `KAGI_GALLERY_STATE=unknown target/debug/examples/lab_i01_s1` — correctly exited 2 and listed valid states

  All 12 PNGs were opened and inspected. PNG paths use brace expansion below:

  ```text
  /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s1/lab-i01-s1-{default,position-rest,position-selected,position-focus}-{dark,light}-en.png
  /Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i01/s1/lab-i01-s1-default-{dark,light}-ja-{900x744,720x692}.png
  ```

- Requirement self-check:
  1. ○ Apple Dark and Light EN captures succeeded.
  2. ○ All available controls use `settings_kit`; only the missing position picker is local.
  3. ○ Required order, categories, labels, subtitle, and switch values match.
  4. ○ Reusable picker takes explicit selection and forced state; uses theme tokens; exposes rest, selected, and focus states.
  5. ○ Kit row geometry, hairlines, radii, and trailing alignment preserved.
  6. ○ Four visibly distinct states, including forced Pressed and Focus.
  7. ○ Commands, PNG paths, and observation limits reported.

- Unclear points: none.

- Discretionary fills: Default window is 900×744 to show all rows with kit heights. Program Activity groups have 10-point spacing. Added JA translations and kit-icon substitutions. `position-rest` shows no selection; `position-selected` shows top-right pressed.

- Kit gaps: Page-local `position_picker`, a 3×2 spatial selector.

- Retries: Three implementation corrections: return type, viewport height, and truncated JA sidebar wording. One initial tool-transport retry. Cargo lock waits were allowed to finish.

- Not observed: Real hover, keyboard focus/navigation, VoiceOver, live selection changes, or scrolling. At 720×692, lower content extends below the viewport; full-height captures show the final off switch.