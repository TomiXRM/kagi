- **Deliverable / evidence:** Created only [examples/lab_i10_s4.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s4.rs), gallery page `lab-i10-s4`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i10_s4.rs
  cargo build --example lab_i10_s4 --features gui-e2e
  target/debug/examples/lab_i10_s4 --list
  rustfmt --edition 2021 --check examples/lab_i10_s4.rs
  ```
  Captured using `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_SIZE`, and `KAGI_GALLERY_OUT`. Also verified unknown-state rejection, inspected every PNG, and measured popup/play alignment with Pillow.

  All 18 PNGs are under:
  `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s4/`

  Filenames:
  `lab-i10-s4-{state}-{theme}.png`, covering all combinations of:
  - States: `default`, `focus-alert-slider`, `focus-mute`, `hover-play`, `volume-minimum`, `volume-middle`, `volume-maximum`
  - Themes: `apple-dark`, `apple-light`

  Additional filenames: `lab-i10-s4-default-{theme}-720x900.png` and `lab-i10-s4-default-{theme}-ja-720x692.png`.

  Default evidence: [Dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s4/lab-i10-s4-default-apple-dark.png), [Light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s4/lab-i10-s4-default-apple-light.png).

- **Requirement self-check:**

  | Checklist | Result |
  |---|---|
  | 1. Dark and Light EN host renders | ○ Both exit successfully; complete content captured. |
  | 2. Kit reuse | ○ All available controls, rows, groups and scaffold reuse the kit. |
  | 3. Reusable slider function | ○ All three use `audio_slider`; minimum, midpoint and maximum inspected. |
  | 4. Reusable device table | ○ Aligned columns, six devices, exactly one theme-role selection. |
  | 5. Full-width choices and Mute | ○ Output selected; checked Mute aligned beneath the trailing end. |
  | 6. Popup/play alignment | ○ Adjacent trailing controls. PNG measurement gives matching centre lines: play fill spans y=180–227; inset chevron spans y=186–221. |
  | 7. Evidence report | ○ Commands, paths and observation limits included. |

  Capture critique: indicators remain on their tracks; sibling controls retain kit styling; trailing controls share the row boundary; both themes retain readable borders and selection. Category tile colours follow the skill.

  [C2] Fixed emoji speaker marks that differed from surrounding symbols and sat low: replaced with theme-coloured vector marks. No remaining C1 findings. The 720×692 captures are **partial**: lower controls and sidebar entries are not captured; complete 900-point-height captures show them.

- **Unclear points:** [Understanding] JA capture meaning / The scenario specifies English content / Treat JA as an environment smoke capture; do not claim Japanese localization.

- **Discretionary fills:** Chose a 900×900 default size, 190-point slider tracks, seven alert ticks, and values 95% / 5% / 50%. Interaction states preserve default data.

  Placement suggestions—reference layout retained:

  | Setting | Pane | Section |
  |---|---|---|
  | Alert sound; effects device; alert volume | Sound | Sound Effects |
  | Startup sound; interface sounds; volume feedback | Sound | Sound Effects |
  | Output/Input; device selection | Sound | Output & Input |
  | Output volume; Mute; Balance | Sound | Output & Input, separate untitled group |

- **Kit gaps:** Reusable device table and speaker-symbol decoration. Slider tracks, ticks, thumbs and captions remain kit-owned.

- **Retries:** One visual revision and recapture; zero build failures. Waited for Cargo’s shared cache lock.

- **Not observed:** Real pointer hover, keyboard focus, VoiceOver, audio playback, live device switching, popup menus or Japanese localization. Captures used forced states and offscreen rendering; no app was activated.