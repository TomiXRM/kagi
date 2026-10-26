- **Deliverable / evidence:** Created only [examples/lab_i07_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s1.rs), gallery page `lab-i07-s1`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i07_s1.rs
  rustfmt --edition 2021 --check examples/lab_i07_s1.rs
  cargo build --example lab_i07_s1 --features gui-e2e
  target/debug/examples/lab_i07_s1 --list
  KAGI_GALLERY_STATE=unknown target/debug/examples/lab_i07_s1
  ```
  Rendered using `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_OUT`, and narrow-size overrides. All 20 captures exited successfully. Unknown-state validation exited 2 and listed valid states. PIL comparisons confirmed interaction-state changes stayed within the affected control.

  PNG directory: [target/ui-lab/runs/i07/s1/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s1/).

  Paths follow these patterns:
  - `lab-i07-s1-<state>-<theme>.png`: all eight states × `dark`, `light`.
  - `lab-i07-s1-default-<theme>-<lang>-720x780.png`: both themes × `en`, `ja`.

  States: `default`, `rest-position`, `selected-top-left`, `selected-top-right`, `selected-bottom-right`, `focus-position`, `hover-position`, `focus-switch`.

  Default evidence: [Apple Dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s1/lab-i07-s1-default-dark.png), [Apple Light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s1/lab-i07-s1-default-light.png).

- **Requirement self-check:**
  1. ○ Both Apple themes render successfully in EN.
  2. ○ All existing control kinds use `settings_kit`; only the spatial picker is local.
  3. ○ Required order, titles, subtitles and values match. Every row is captured; the final switch is off.
  4. ○ Reusable picker accepts selected cell and forced state, uses theme tokens, and exposes rest, selected and focus states.
  5. ○ Kit row geometry, hairlines, radii and trailing alignment remain intact. Checks occupy their selected cells at indices 0, 2 and 5.
  6. ○ Focus-position, hover-position and focus-switch visibly differ from default. Interaction states preserve all values and text.
  7. ○ Commands, PNG paths and observation limits are reported.

  Capture critique: all 20 PNGs opened. `[C1]` None remaining. `[C2]` Picker check contrast differed from surrounding accent controls; changed its foreground to window ink and recaptured. Controls remain consistent within each role, trailing bounds align, category hues follow the skill, and both themes retain readable controls and borders.

- **Unclear points:** none.

- **Discretionary fills:** Used 900×780 for complete coverage and 720×780 for narrow captures. Used `Pause` for Blocked, the nearest supplied symbol. Terminal and Shortcuts share Gray because both map to that category.

  Placement:

  | Setting | Pane | Section |
  |---|---|---|
  | Notification position | Notifications | In-App Notifications |
  | Show activity in tabs; system notifications; Dock badge; sounds | Notifications | Program Activity |
  | Working; Blocked; Error; Done navigation | Notifications | Program Activity |
  | Include idle processes; show each process | Notifications | Program Activity |

- **Kit gaps:** Page-local `position_picker(selected, forced)` implements the 3×2 spatial preview. One row-major mapping places cells and their contained checks.

- **Retries:** Two correction cycles: fixed the picker’s `Stateful<Div>` return type, then improved check contrast and recaptured. One image-preview retry used supported detail after `low` was rejected.

- **Not observed:** Real pointer hover, keyboard focus, VoiceOver or persisted setting changes. JA environment captures succeeded, but page labels remain English, so Japanese text width was not verified. No app was activated or brought forward.