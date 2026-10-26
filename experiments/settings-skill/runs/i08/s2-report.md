- **Deliverable / evidence:** [examples/lab_i08_s2.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s2.rs), gallery page `lab-i08-s2`.

  Commands run: `rustfmt --edition 2021 examples/lab_i08_s2.rs`, file-only `rustfmt --check`, `cargo build --example lab_i08_s2 --features gui-e2e`, binary `--list`, and gallery captures. Unknown-state invocation correctly exited 2.

  All 16 PNGs are under [target/ui-lab/runs/i08/s2/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s2/), using these filename expansions:
  `{default,hover-choice,focus-choice,focus-slider,icons-minimum,icons-maximum}-{dark,light}-en.png`
  and `default-{dark,light}-{en,ja}-720.png`.

- **Requirement self-check:**

  1. ○ Both Apple themes rendered successfully in EN.
  2. ○ All existing control kinds come from the kit.
  3. ○ Reusable preview choice function; selected labels emphasized, accent rings visible, clusters share one divided group.
  4. ○ Kit segmented controls show equal-width text choices and selection.
  5. ○ Five slider ticks, trailing `3`, and kit pop-up values.
  6. ○ Kit geometry retained; all rows visible, trailing controls aligned, both themes readable. Slider endpoints and midpoint align with ticks. `[C1]` None remaining. `[C2]` Preview corner mismatch fixed. Japanese sidebar ellipsis retains the kit’s behavior; the full page title remains visible.
  7. ○ Commands, paths and observation limits reported.

- **Unclear points:** `[Execution] Accessibility role unavailable on plain Div / GPUI requires a stateful element / Assign an ID before accessibility properties.` Resolved; none outstanding.

- **Discretionary fills:** Added Japanese labels, simplified preview artwork, and slider endpoint states representing values 1–5.

  Placement suggestions—specified layout retained:

  | Setting | Pane | Section |
  |---|---|---|
  | Color scheme, Interface style, Tint with theme | Interface | Appearance |
  | Tab style, Fill available space, Show tab icons, Use color for tab icons, Shell icon theme | Interface | Tabs |
  | Pane style, Show pane headers | Interface | Panes |

- **Kit gaps:** `choice_tile(preview, label, selected, forced)` and its miniature preview artwork.

- **Retries:** 3 correction rounds: missing element ID, preview corners, and wrapper sizing compliance. Final captures match the delivered source.

- **Not observed:** Real hover, keyboard focus, VoiceOver, persistence or live setting changes. Forced states were rendered offscreen; no app was activated.