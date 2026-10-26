- **Deliverable / evidence:** Created only [examples/lab_i05_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i05_s5.rs), gallery page `lab-i05-s5`.

  Commands run: `rustfmt --edition 2021 examples/lab_i05_s5.rs`, its `--check` equivalent, `cargo build --example lab_i05_s5`, `--list`, unknown-state validation (correctly exited 2), and gallery captures through the binary and `cargo run --example lab_i05_s5`.

  Captures used display `ad1ab626`, both Apple themes, all four states, Japanese, and narrow sizes. All 16 PNGs were opened and inspected under `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i05/s5/`:
  - `lab-i05-s5-{default,focus-card,hover-manage,focus-nav}-{dark,light}.png`
  - `lab-i05-s5-default-ja-{dark,light}.png`
  - `lab-i05-s5-narrow-{en,ja}-{dark,light}.png`
  - `lab-i05-s5-narrow-tall-ja-{dark,light}.png`

- **Requirement self-check:**
  1. ○ Both EN themes render through `cargo run`, write PNGs, and exit 0.
  2. ○ Kit controls reused throughout.
  3. ○ Reusable card function; equal columns and heights; wrapped Japanese titles remain clear of subtitles.
  4. ○ Reusable 93% progress bar inside storage; Manage… aligns with other trailing buttons.
  5. ○ Off/On appear before their chevrons.
  6. ○ Green status dot and See All are visible.
  7. ○ Commands, paths, and observation limits reported.

  Capture critique: fill stays inside its track; matching controls have consistent geometry; trailing buttons align; sidebar tiles use `Tint::Accent`; both themes remain readable. No remaining C1/C2 findings in the full-height captures. **[C2] Fixed:** collapsed section headings after adding a nested flex wrapper; restored direct kit scaffold composition.

- **Unclear points:** `[Execution] Nested wrapper collapsed headings / Additional flex sizing context / Preserve the kit scaffold and inspect native captures after layout changes.`

- **Discretionary fills:** Used supported approximate symbols, added EN/JA text and three forced interaction states, selected a 1000×1040 default size, and anchored help at the bottom trailing corner.

- **Kit gaps:** Determinate progress bar, circular account badge, and leading section status dot. Cards/grids compose kit groups, rows, and icon tiles.

- **Retries:** Three layout/capture revision rounds; zero capture failures. Cargo lock waits were respected.

- **Not observed:** Real hover, keyboard focus, VoiceOver, or action behavior. The 720×692 captures show partial content; lower cards and navigation were captured separately at full height.