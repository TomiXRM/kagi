- **Deliverable / evidence:** [examples/lab_i08_s4.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s4.rs), gallery page `lab-i08-s4`. Only this source file was written.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i08_s4.rs
  rustfmt --edition 2021 --check examples/lab_i08_s4.rs
  cargo build --example lab_i08_s4 --features gui-e2e
  target/debug/examples/lab_i08_s4 --list
  ```
  Captures used `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_OUT`, and size overrides. Unknown-state rejection returned exit 2 and listed valid states.

  All 20 PNGs were opened and reviewed under [target/ui-lab/runs/i08/s4/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s4):
  `default`, `focus-alert-volume`, `focus-mute`, `hover-play`, `focus-output`, `volume-min`, `volume-middle`, `volume-max`, `default-ja-narrow`, and `default-720x692`, each suffixed `-dark.png` and `-light.png`.

- **Requirement self-check:**

  | Item | Result |
  |---|---|
  | 1. Dark/Light EN host render | ○ Both exited 0; default captures show every required item. |
  | 2. Kit reuse | ○ Sections, groups, rows, switches, segments, pop-ups, checkbox, tiles and buttons use the kit. |
  | 3. Reusable slider | ○ One function delegates geometry to kit `Slider`; minimum, midpoint and maximum inspected. |
  | 4. Reusable device table | ○ Aligned headers/columns, six devices, exactly one selection using `ink::selected()`. |
  | 5. Full-width selector and Mute | ○ Output visibly selected; checked Mute aligns beneath the slider’s trailing end. |
  | 6. Alert pop-up and play | ○ Together at the common trailing edge. |
  | 7. Evidence report | ○ Commands, paths and observation limits included. |

  `[C1]` Collapsed section headings fixed with full-width wrappers and no-wrap headings. `[C2]` Table header alignment corrected. Both themes retain visible controls and borders.

- **Unclear points:** `[Execution]` Section headings collapsed / Width wrappers alone were insufficient / Preserve full-width wrappers and control heading wrapping separately from body text.

- **Discretionary fills:** Default size `940×960`; slider values `95%`, `4%`, `50%`; nearest available sidebar icons; Japanese labels.

  **Placement suggestions**—requested layout preserved:

  | Setting | Pane | Section |
  |---|---|---|
  | Alert sound, playback device, alert volume, sound toggles | Sound | Sound Effects |
  | Output/Input and device table | Sound | Output & Input |
  | Output volume, Mute, Balance | Sound | Following unlabelled group |
  | Help | Sound | Bottom trailing corner |

- **Kit gaps:** Reusable device table composed from kit Rows; speaker-symbol ends around kit sliders because `IconName` lacks speaker variants. All three sliders share the same wrapper.

- **Retries:** Three implementation passes after the initial build: warning/alignment cleanup, width-wrapper attempt, then heading-wrap correction. Cargo locks were waited out.

- **Not observed:** Real hover, keyboard focus, VoiceOver, audio playback or device interaction. The `720×692` captures are partial: lower devices, volume group and Help are **not captured** there; they are visible in the default and taller Japanese captures.