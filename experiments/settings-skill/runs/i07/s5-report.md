- Deliverable / evidence: [examples/lab_i07_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s5.rs), gallery page `lab-i07-s5`. Only this source file was written.

  Commands run:

  ```sh
  rustfmt --edition 2021 examples/lab_i07_s5.rs
  rustfmt --check --edition 2021 examples/lab_i07_s5.rs
  cargo build --example lab_i07_s5 --features gui-e2e
  target/debug/examples/lab_i07_s5 --list
  ```

  Captures used `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, `KAGI_GALLERY_OUT`, and optional `KAGI_GALLERY_SIZE`. All 20 captures exited 0 and were opened. An unknown-state invocation exited 2 and listed valid states.

  PNG directory: [target/ui-lab/runs/i07/s5/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s5).

  Exact filename pattern: `lab-i07-s5-{suffix}-{dark,light}.png`, for these suffixes: `default`, `hover-card`, `pressed-card`, `focus-card`, `disabled-card`, `focus-manage`, `hover-nav`, `default-720x692`, `default-720x1000`, `default-ja`.

- Requirement self-check:

  | Checklist | Result |
  |---|---|
  | 1. Apple Dark and Light EN render | ○ Both succeed; default captures show all content. |
  | 2. Kit reuse | ○ Sections, groups, rows, navigation, tiles, capsules and progress use the kit. |
  | 3. Reusable cards and equal grids | ○ Both grids have three equal columns and equal-height cards. Wrapped titles remain separate from subtitles. |
  | 4. Reusable progress and storage alignment | ○ Uses kit `progress_bar(0.93, Tint::Success)`; Manage… aligns with See All and Help. |
  | 5. Navigation values | ○ Off and On appear before their chevrons. |
  | 6. Saved-section header | ○ Green dot and trailing See All are visible. |
  | 7. Evidence and limits reported | ○ Commands, paths and unobserved behavior included. |

  Capture critique: the progress fill stays inside its track; focus rings surround their intended parts; card geometry is consistent; category hues follow the skill; both themes retain readable text and boundaries.

- Unclear points:

  `[Execution] [C1] Collapsed headings / Nested wrappers lost intrinsic width / Preserve width and flex allocation; return the kit section directly to the detail column. Fixed.`

  `[Execution] [C1] Missing See All / Accessory could shrink / Keep the capsule from shrinking. Fixed.`

  `[Execution] [C2] Missing light card borders / Transparent focus-ring border replaced the group border / Wrap the kit group with the ring. Fixed.`

  No unresolved C1/C2 findings.

- Discretionary fills: Default size is `900×1000`; cards use a shared 100-point height. Symbols use the nearest permitted `IconName`. English scenario text remains English in JA-locale captures.

  | Content | Pane | Section |
  |---|---|---|
  | Account identity; storage | iCloud | Header; Storage |
  | Photos, Drive, Passwords, Notes, Messages, Mail | iCloud | Saved to iCloud |
  | Manage Plan, Family, Apple Invitations, Private Relay, Hide My Email, Custom Email Domain | iCloud | iCloud+ Features |
  | Protection; web access | iCloud | Final navigation group |
  | Help | iCloud | Bottom trailing corner |

- Kit gaps: `cloud_card` summary layout, `account_badge` round symbol, and `saved_section` leading status-dot adapter. Progress required no local implementation.

- Retries: **5**—one compiler correction and four visual correction rounds. Cargo locks were allowed to finish.

- Not observed: Real pointer hover, keyboard focus, VoiceOver, or action behavior. The `720×692` captures are partial: lower features, navigation and Help are not captured there; both `720×1000` captures show them fully. No app was activated or brought forward.