- Deliverable / evidence: Created only [examples/lab_i03_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i03_s1.rs), gallery page `lab-i03-s1`.
  
  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i03_s1.rs
  cargo build --example lab_i03_s1
  target/debug/examples/lab_i03_s1 --list
  rustfmt --edition 2021 --check examples/lab_i03_s1.rs
  KAGI_GALLERY_STATE=unknown target/debug/examples/lab_i03_s1
  ```
  Captures used `KAGI_GALLERY_DISPLAY=ad1ab626`, `KAGI_GALLERY_PAGE=lab-i03-s1`, both Apple themes, and all five states. Additional default captures used `KAGI_GALLERY_SIZE=720x692` with EN and JA. All 14 final captures exited 0; unknown state exited 2 and listed valid states.
  
  PNG directory: [target/ui-lab/runs/i03/s1/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i03/s1/). Files: `{default,rest,selected,hover,focus}-{dark,light}.png` and `default-{en,ja}-720-{dark,light}.png`.
  
  Default evidence: [Dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i03/s1/default-dark.png), [Light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i03/s1/default-light.png).

- Requirement self-check:
  
  1. ○ Native host rendered Apple Dark and Apple Light in EN.
  2. ○ All existing control kinds use `settings_kit`.
  3. ○ Required ordering, labels, subtitle, status rows, and switch values match.
  4. ○ Reusable picker accepts selected cell and forced state; uses theme tokens; exposes rest, selected, and focus.
  5. ○ Kit owns row heights, dividers, radii, and trailing slots. No row geometry overrides.
  6. ○ `default`, `rest`, `selected`, `hover`, and `focus` visibly differ.
  7. ○ Commands, PNG paths, and observation limits reported.
  
  Opened every final PNG. Indicators remain inside their cells/tracks; controls of each kind are consistent; sidebar tiles follow the skill; both themes remain legible. **[C1] Fixed:** resting picker cells previously disappeared into the track. **[C2] None:** no remaining sibling-control inconsistency.

- Unclear points: [Execution] Initial icon names were unavailable / The component icon catalog is limited / Inspect the installed catalog before selecting variants.

- Discretionary fills: Default window is 900×760 so all content fits. Added Japanese labels, an asterisk shortcut symbol, and unselected/top-left picker previews. At 720×692, lower content requires scrolling; the long Japanese Interface label uses kit ellipsis.

- Kit gaps: Page-local `position_picker`, a 3×2 spatial preview with selection check and forced-state rendering.

- Retries: 3 correction rounds: two compilation retries for icon names/return type, then one recapture round after fixing picker contrast and increasing window height.

- Not observed: Real hover, keyboard focus, VoiceOver, click behavior, or scrolling. Interaction captures draw forced states. No app was activated or brought forward.