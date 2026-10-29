- **Deliverable / evidence:** Created only [examples/lab_i10_s6.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s6.rs), page `lab-i10-s6`.
  - Commands: `rustfmt --edition 2021 examples/lab_i10_s6.rs`; `cargo build --example lab_i10_s6 --features gui-e2e`; binary `--list`; gallery captures; unknown-state rejection check; file-scoped formatting check; read-only PNG measurements.
  - PNG directory: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s6/`
  - Twelve PNGs, expressed with brace expansion:
    - `lab-i10-s6-{default,hover-card,focus-card,scroll-middle,scroll-end}-{apple-dark,apple-light}-en.png`
    - `lab-i10-s6-default-{apple-dark,apple-light}-ja-720.png`

- **Requirement self-check:**
  1. ○ Both EN host themes render successfully.
  2. ○ All standard controls come from `settings_kit`.
  3. ○ Reusable `theme_card(theme, selected, forced)` uses theme-owned preview colours; no hex literals. Three corresponding preview interiors were pixel-identical between host themes.
  4. ○ All 15 built-in themes appear once, filtered by `dark`, preserving registry order; all were captured.
  5. ○ Apple selections have accent rings and emphasized names. Stable-ID strips scroll horizontally.
  6. ○ Commands, capture paths, and observation limits reported.
  - **[C1] Fixed:** Long theme name clipped; increased uniform card height and set explicit label line height.
  - **[C2] Fixed:** Some `~/` chips blended into their backgrounds; used each theme’s accent and terminal colours.
  - Final critique: aligned preview heights, contained indicators, readable borders, and matching switch trailing edges. Both switch tracks measured **56×32 physical pixels**.

- **Unclear points:** none.

- **Discretionary fills:** Chose sidebar destinations, card dimensions, and capture states. Placement suggestions preserve the specified layout:

  | Setting | Pane | Section |
  |---|---|---|
  | Light theme and extended palette | Themes | Light Mode |
  | Dark theme and extended palette | Themes | Dark Mode — Active |

- **Kit gaps:** Visual theme-choice card and its horizontal preview strip.

- **Retries:** One visual refinement and recapture cycle; one read-only measurement helper rerun after correcting a missing import. Both builds passed.

- **Not observed:** Real hover, keyboard focus, VoiceOver, wheel scrolling, or action execution. Interaction captures use forced states. Tests, Clippy, and workspace formatting were not run, as requested.