- **Deliverable / evidence:** Created only [examples/lab_i04_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i04_s2.rs), page `lab-i04-s2`.
  
  Commands run: `rustfmt --edition 2021 examples/lab_i04_s2.rs`, its `--check` variant, `cargo build --example lab_i04_s2`, `cargo run --example lab_i04_s2`, binary `--list`, unknown-state rejection, and gallery capture commands using display `ad1ab626`.
  
  All 18 final PNGs were opened and inspected under [target/ui-lab/runs/i04/s2/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i04/s2):
  - `lab-i04-s2-{default,hover-choice,pressed-choice,focus-choice,disabled-choice,focus-slider,focus-switch}-{dark,light}.png`
  - `lab-i04-s2-default-{en,ja}-720-{dark,light}.png`
  
  Default captures: [Dark](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i04/s2/lab-i04-s2-default-dark.png), [Light](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i04/s2/lab-i04-s2-default-light.png).

- **Requirement self-check:**
  1. ○ Apple Dark and Light native renders succeeded.
  2. ○ Standard controls and layout components reuse the kit.
  3. ○ Reusable choice tiles; selected rings and emphasized labels; shared group with vertical divider.
  4. ○ Kit text-only, equal-width segments show Above and Comfortable selected.
  5. ○ Five slider ticks, aligned thumb at value `3`, trailing value, kit pop-ups.
  6. ○ Kit rows, hairlines, radii and trailing alignment preserved. Both themes read clearly.
  7. ○ Commands, capture paths and observation limits reported.
  
  `[C1]` No remaining findings. `[C2]` Corrected inverted Light/Dark preview fills and strengthened faint slider ticks and thumb borders. Interaction states preserve all default values.

- **Unclear points:** `[Execution] Hidden-window capture failures / Code covered the display / Require a visible gallery window and retry without activation.` Resolved when visibility returned.

- **Discretionary fills:** Schematic previews, Japanese translations, ellipsis icon for Shortcuts, and seven capture states. The Japanese sidebar label elides through the kit; the full pane title remains visible.

- **Kit gaps:** Page-local `choice_tile` with miniature previews, and `tick_slider`.

- **Retries:** Four failed build invocations for IDs, unavailable icons and type inference; three rejected occluded captures; two visual refinement rounds. Final captures all succeeded.

- **Not observed:** Real hover, keyboard focus/navigation, VoiceOver or persistent setting changes. No tests, clippy, global formatting or app activation performed.