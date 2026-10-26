- **Deliverable / evidence:** Created only [examples/lab_i10_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s2.rs), gallery page `lab-i10-s2`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i10_s2.rs
  cargo build --example lab_i10_s2 --features gui-e2e
  target/debug/examples/lab_i10_s2 --list
  rustfmt --edition 2021 --check examples/lab_i10_s2.rs
  KAGI_GALLERY_STATE=not-a-state target/debug/examples/lab_i10_s2
  ```
  Captures used:
  ```sh
  KAGI_GALLERY_THEME=apple-{dark,light} \
  KAGI_GALLERY_LANG={en,ja} \
  KAGI_GALLERY_STATE=<state> \
  KAGI_GALLERY_OUT=<png-path> target/debug/examples/lab_i10_s2
  ```
  Also captured default EN/JA with `KAGI_GALLERY_SIZE=720x692`. All 24 captures succeeded and were opened.

  PNG directory: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s2/`

  Filenames:
  - `lab-i10-s2-{default,hover-choice,pressed-choice,focus-choice,disabled-choice,focus-switch,focus-slider,icons-min,icons-max}-{dark,light}.png`
  - `lab-i10-s2-default-ja-{dark,light}.png`
  - `lab-i10-s2-default-720-{en,ja}-{dark,light}.png`

- **Requirement self-check:**
  1. ○ Apple Dark and Light EN renders succeed; every requested row is visible.
  2. ○ All listed controls and containers use `settings_kit`.
  3. ○ Reusable choice tile accepts preview, label, selection, and forced state. Selected labels are emphasized; accent rings and shared-group divider are visible.
  4. ○ Kit segments use equal-width text choices; Above and Comfortable are selected.
  5. ○ Five ticks, trailing value `3`, and kit pop-ups are visible. Endpoint captures show `1` and `5`.
  6. ○ Kit geometry retained. `[C2]` Unequal preview widths were corrected by accounting for cluster gaps. `[C2]` Slider/value alignment was corrected by balancing the tick inset. `[C2]` Auto preview contrast was strengthened. No open findings.
  7. ○ Commands, PNG paths, and observation limits reported. Invalid state exits 2 and lists valid states.

- **Unclear points:** `[Understanding] Slider range unspecified / Five ticks and default 3 supplied without endpoints / Declare one range and derive both thumb position and caption from its value.`

- **Discretionary fills:** Used range 1–5, symbolic previews, category-colored sidebar icons, and Japanese translations.

  Placement suggestions—retained the prescribed layout:

  | Setting | Pane | Section |
  |---|---|---|
  | Color scheme | Interface | Appearance |
  | Interface style | Interface | Appearance |
  | Tint with theme | Interface | Appearance |
  | Tab style | Interface | Tabs |
  | Fill available space | Interface | Tabs |
  | Show tab icons | Interface | Tabs |
  | Use color for tab icons | Interface | Tabs |
  | Shell icon theme | Interface | Tabs |
  | Pane style | Interface | Panes |
  | Show pane headers | Interface | Panes |

- **Kit gaps:** `choice_tile` and its miniature preview illustration. No existing kit control was reimplemented.

- **Retries:** Two revision rounds: one compile correction adding IDs before accessibility roles; one visual correction round followed by recapture. Cargo lock waits were respected.

- **Not observed:** Real hover, keyboard focus/navigation, VoiceOver, or preference persistence. Interaction states were forced offscreen renders. No foreground activation, tests, or Clippy.