- **Deliverable / evidence:** Implemented only [examples/lab_i09_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s1.rs), gallery page `lab-i09-s1`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i09_s1.rs
  cargo build --example lab_i09_s1 --features gui-e2e
  target/debug/examples/lab_i09_s1 --list
  KAGI_GALLERY_STATE=invalid target/debug/examples/lab_i09_s1
  rustfmt --edition 2021 --check examples/lab_i09_s1.rs
  ```
  Captured through the binary using `KAGI_GALLERY_THEME`, `LANG`, `STATE`, `SIZE`, and `OUT`. Unknown-state validation correctly exited 2. Build and formatting passed.

  All 24 PNGs are in [target/ui-lab/runs/i09/s1/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s1/):
  - `lab-i09-s1-<state>-<theme>-en.png`: all nine listed states × `apple-dark` / `apple-light`.
  - `lab-i09-s1-default-<theme>-ja.png`: both themes; labels remain English.
  - `lab-i09-s1-default-<theme>-en-720x692.png`: diagnostic captures, last row clipped.
  - `lab-i09-s1-default-<theme>-en-720x740.png`: complete narrow captures.

  Default evidence: [Dark PNG](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s1/lab-i09-s1-default-apple-dark-en.png), [Light PNG](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s1/lab-i09-s1-default-apple-light-en.png).

- **Requirement self-check:**

  | Item | Result |
  |---|---|
  | 1. Dark and Light host render | ○ Both succeeded offscreen. |
  | 2. Kit reuse | ○ All existing control kinds use the kit. |
  | 3. Structure and content | ○ Reference order, subtitle, statuses, and final off switch preserved. |
  | 4. Reusable position picker | ○ Explicit selection/forced parameters; shared position mapping checked at indices 0, 2, 3, and 5. |
  | 5. Kit geometry | ○ Kit rows unchanged; picker height 24 points. Picker and switch bounds share the trailing edge within each theme. |
  | 6. Visible interaction states | ○ `focus-picker`, `hover-picker`, and `focus-switch` differ from default without changing data. |
  | 7. Evidence report | ○ Commands, paths, limitations included. |

  Opened every PNG. Cells have equal dimensions, checks remain inside selected cells, and both themes read clearly. **[C1]** None remaining. **[C2]** Corrected the picker check’s contrast to match the inverse treatment of nearby switches; refreshed all captures.

- **Unclear points:**  
  `[Execution] 720×692 clips the final row / Content exceeds viewport height / Increase capture height and verify again.` Complete evidence uses 740-point height.

- **Discretionary fills:** Used a 900×740 default canvas and nearest available symbols for Rex-specific icons. Kept both Terminal and Shortcuts gray according to the hue table.

  **Placement suggestions** — fixed reference layout retained:

  | Setting | Pane | Section |
  |---|---|---|
  | Notification position | Notifications | In-App Notifications |
  | Four notification switches | Notifications | Program Activity |
  | Working, Blocked, Error, Done | Notifications | Program Activity |
  | Idle processes and per-process tabs | Notifications | Program Activity |

- **Kit gaps:** Page-local `position_picker(selected_cell, forced)`: a 3×2 visual picker using theme tokens and `metrics::CONTROL_HEIGHT`.

- **Retries:** Three corrective iterations: return-type compile fix, taller narrow capture, and picker check contrast correction.

- **Not observed:** Real hover, keyboard focus/navigation, VoiceOver, persisted setting changes, or Japanese translation. Interaction evidence uses forced states. No app was activated; tests, Clippy, and workspace-wide formatting were not run.