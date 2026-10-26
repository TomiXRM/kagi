- Deliverable / evidence: Created only [examples/lab_i08_s6.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s6.rs), gallery page `lab-i08-s6`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i08_s6.rs
  rustfmt --edition 2021 --check examples/lab_i08_s6.rs
  cargo build --example lab_i08_s6 --features gui-e2e
  target/debug/examples/lab_i08_s6 --list
  KAGI_GALLERY_STATE=unknown target/debug/examples/lab_i08_s6
  ```
  Captures used `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_SIZE`, and `KAGI_GALLERY_OUT` with the built binary.

  All 14 PNGs are under [target/ui-lab/runs/i08/s6/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s6/):
  ```text
  lab-i08-s6-{default,hover-card,pressed-card,focus-card,disabled-card}-{dark,light}.png
  lab-i08-s6-all-themes-{dark,light}.png
  lab-i08-s6-narrow-ja-{dark,light}.png
  ```
  These patterns enumerate all filenames. Standard captures are 900×692, all-theme captures 1900×692, and narrow Japanese-locale captures 720×692.

- Requirement self-check:  
  1. ○ Apple Dark and Apple Light EN renders succeeded.  
  2. ○ All available control kinds use the kit; no local row geometry.  
  3. ○ Reusable `theme_card(theme, selected, forced)` uses theme fields for preview colours; no hex literals. Previews retain their colours across host themes.  
  4. ○ All 15 built-ins appear exactly once: six light, nine dark, preserving `themes_by_name()` order. All are visible in the wide captures.  
  5. ○ Apple selections have accent rings and bold names. Fixed-width strips have stable IDs and horizontal overflow.  
  6. ○ Commands, PNG locations, and observation limits are reported.

  Capture critique: indicators are contained, card geometry is consistent, switch trailing edges align, category hues follow the skill, and both themes remain readable. No open [C1] findings. [C2] The longest name initially wrapped its closing parenthesis alone; widening cards from 148 to 164 points fixed it.

- Unclear points: none.

- Discretionary fills: Added representative Kagi sidebar panes and forced states targeting Catppuccin Mocha. Placement suggestions preserve the requested layout:

  | Setting | Pane | Section |
  |---|---|---|
  | Light theme / extended palette | Themes | Light Mode |
  | Dark theme / extended palette | Themes | Dark Mode — Active |

- Kit gaps: Visual theme-choice card with terminal sample and eight ANSI dots. Dot placement shares one mapping, checked at minimum, middle, and maximum indices.

- Retries: One visual revision and recapture for name wrapping. Builds succeeded; shared Cargo locks were allowed to clear.

- Not observed: Real hover, keyboard focus, VoiceOver, manual horizontal scrolling, or persistence. Interaction captures use forced states. No app was activated or brought forward.