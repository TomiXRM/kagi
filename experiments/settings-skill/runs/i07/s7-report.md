- **Deliverable / evidence:** [examples/lab_i07_s7.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s7.rs), gallery page `lab-i07-s7`. Only this source file was written.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i07_s7.rs
  cargo build --example lab_i07_s7 --features gui-e2e
  target/debug/examples/lab_i07_s7 --list
  rustfmt --edition 2021 --check examples/lab_i07_s7.rs
  KAGI_GALLERY_STATE=not-a-state target/debug/examples/lab_i07_s7
  ```
  Captures ran through `target/debug/examples/lab_i07_s7` with `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_OUT`, and narrow captures with `KAGI_GALLERY_SIZE=720x692`.

  All ten PNGs are under [target/ui-lab/runs/i07/s7/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s7):
  ```text
  lab-i07-s7-default-dark-en.png
  lab-i07-s7-default-light-en.png
  lab-i07-s7-focus-name-dark-en.png
  lab-i07-s7-focus-name-light-en.png
  lab-i07-s7-focus-swatch-dark-en.png
  lab-i07-s7-focus-swatch-light-en.png
  lab-i07-s7-default-dark-en-720x692.png
  lab-i07-s7-default-light-en-720x692.png
  lab-i07-s7-default-dark-ja-720x692.png
  lab-i07-s7-default-light-ja-720x692.png
  ```

- **Requirement self-check:**
  1. ○ Apple Dark and Apple Light EN renders exited successfully.
  2. ○ Standard controls, rows, groups, sections, sidebar items and icon tiles use the kit.
  3. ○ One reusable swatch function; computed hex values; aligned columns; all 22 entries captured.
  4. ○ Reusable name-field look with visible `focus-name` state.
  5. ○ Exactly one primary button; Cancel immediately precedes it; Revert… sits separately at the leading edge.
  6. ○ Dimmed Themes backdrop; rounded sheet surface and shadow.
  7. ○ Commands, PNG paths and observation limits reported.

  Capture critique: **[C1] fixed** collapsed palette headings by making their full-width wrapper a vertical layout. Final captures show contained focus rings, consistent swatches, aligned palette edges, category-coloured tiles and readable borders in both appearances. No unresolved [C1] or [C2] findings.

- **Unclear points:** [Understanding] Cursor Text and Selected Text have no dedicated theme fields / The theme schema supplies neither token / Reuse Apple Dark’s `term_bg` and `term_fg`, respectively, without inventing palette values.

- **Discretionary fills:** Default size is 900×740. Selection retains alpha and displays eight-digit hex. The primary action extends the kit capsule’s appearance.

  | Setting | Pane | Section |
  |---|---|---|
  | Theme selection | Themes | Built-in / Custom |
  | Name and appearance badge | Themes editor | Header |
  | Preview and extended palette | Themes editor | Colors |
  | Six terminal colours | Themes editor | Terminal |
  | Eight standard colours | Themes editor | ANSI Standard |
  | Eight bright colours | Themes editor | ANSI Bright |

- **Kit gaps:** Sheet/scrim, text-field look, colour swatch and decorative appearance badge. No replacement standard controls were introduced.

- **Retries:** 2: one compile correction (`font_family`, stateful dialog element), then one visual correction for collapsed headings. Three build attempts total.

- **Not observed:** Real hover, keyboard focus, text editing, VoiceOver or functional save/revert behaviour. Focus captures use forced states. No app was activated; tests and clippy were not run, as requested.