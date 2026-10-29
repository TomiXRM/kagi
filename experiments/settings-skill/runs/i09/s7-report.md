- **Deliverable / evidence:** Created only [examples/lab_i09_s7.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s7.rs), gallery page `lab-i09-s7`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i09_s7.rs
  rustfmt --edition 2021 --check examples/lab_i09_s7.rs
  cargo build --example lab_i09_s7 --features gui-e2e
  target/debug/examples/lab_i09_s7 --list
  ```
  Captured `default`, `focus-name`, and `focus-swatch` with `KAGI_GALLERY_THEME=apple-dark|apple-light`, `KAGI_GALLERY_LANG=en`, and `KAGI_GALLERY_OUT`. Also captured `focus-name` with `KAGI_GALLERY_LANG=ja KAGI_GALLERY_SIZE=720x692` in both themes. Unknown-state invocation correctly exited 2 and listed valid states. Used Pillow for pixel measurements and state-difference bounds. No tests, clippy, or workspace formatting ran.

  PNG paths under `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s7/`:
  ```text
  lab-i09-s7-default-apple-dark.png
  lab-i09-s7-default-apple-light.png
  lab-i09-s7-focus-name-apple-dark.png
  lab-i09-s7-focus-name-apple-light.png
  lab-i09-s7-focus-swatch-apple-dark.png
  lab-i09-s7-focus-swatch-apple-light.png
  lab-i09-s7-focus-name-apple-dark-ja-720x692.png
  lab-i09-s7-focus-name-apple-light-ja-720x692.png
  ```

- **Requirement self-check:**

  | Item | Result |
  |---|---|
  | 1. Host render | ○ Both Apple themes rendered successfully in EN; all eight PNGs opened. |
  | 2. Kit reuse | ○ All available control kinds use `settings_kit`; local gaps listed below. |
  | 3. Swatches | ○ Reusable color/label/state function; computed hex captions; aligned 6/8/8 columns. |
  | 4. Name field | ○ Reusable field look with visible `focus-name` state. |
  | 5. Footer | ○ Exactly one primary button; Cancel immediately precedes Save & Apply; Revert… leads. |
  | 6. Sheet | ○ Dimmed Themes page, rounded independent surface, border and shadow. |
  | 7. Report | ○ Commands, paths, limitations and retries recorded. |

  Capture critique: indicators remain inside their cells; interaction-state differences are confined to the named field or swatch. Measured control heights are 48 physical pixels. Labels, borders and white swatches remain visible in both themes; all palette entries and footer actions fit the narrow captures.

  Fixed findings: **[C1]** collapsed name text → flexible text width; **[C2]** misaligned appearance pill → field alignment; **[C2]** light-theme trailing-edge discrepancy → accessory inset matching the kit border. No open findings.

- **Unclear points:**  
  [Understanding] Separate Cursor Text and Selected Text fields are absent / Theme supplies no dedicated tokens / Reuse `term_bg` and `term_fg` explicitly.  
  [Execution] GPUI API mismatches / Accessibility requires stateful elements; font and shadow signatures differ / Follow the checked-out public API.  
  [Formatting] Name collapsed to ellipsis / Truncated child lacked flexible width / Use `flex_1()` and `min_w_0()`.

- **Discretionary fills:** Used a 1000×780 default canvas and three built-in theme previews. Selection preserves alpha as `#0091FF66`, composited over terminal background. JA-environment captures retain the scenario’s English copy.

  **Placement suggestions** — reference layout retained:

  | Setting | Pane | Section |
  |---|---|---|
  | Theme selection, system appearance | Themes | Built-in Themes |
  | Custom theme, import | Themes | Custom Themes |
  | Name, appearance badge | Themes | Editor identity |
  | Preview, terminal and ANSI colors, extended palette | Themes | Colors |
  | Revert, Cancel, Save & Apply | Themes | Editor footer |

- **Kit gaps:** Page-local swatch, text-field look, appearance pill, visual theme-choice tile, and sheet/scrim surface. Footer actions remain kit capsule buttons.

- **Retries:** Four correction rounds: two compilation fixes and two capture-driven layout fixes. Cargo lock waits were allowed to finish.

- **Not observed:** Real hover, keyboard focus, VoiceOver, text editing, color picking, or action execution. Focus is forced gallery styling. No app was activated or brought forward.