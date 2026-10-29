- **Deliverable / evidence:** [examples/lab_i09_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s5.rs), gallery page `lab-i09-s5`. Only this source file was written.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i09_s5.rs
  cargo build --example lab_i09_s5 --features gui-e2e
  target/debug/examples/lab_i09_s5 --list
  rustfmt --edition 2021 --check examples/lab_i09_s5.rs
  ```
  Captured with `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, and `KAGI_GALLERY_OUT`; Japanese captures additionally used `KAGI_GALLERY_SIZE=720x1040`. An unknown state correctly exited nonzero and listed valid states. Pillow inspected PNG geometry.

  All ten PNGs were opened. Directory: [target/ui-lab/runs/i09/s5/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i09/s5/).
  ```text
  lab-i09-s5-default-apple-dark.png
  lab-i09-s5-focus-card-apple-dark.png
  lab-i09-s5-hover-manage-apple-dark.png
  lab-i09-s5-focus-navigation-apple-dark.png
  lab-i09-s5-default-apple-dark-ja-720.png
  lab-i09-s5-default-apple-light.png
  lab-i09-s5-focus-card-apple-light.png
  lab-i09-s5-hover-manage-apple-light.png
  lab-i09-s5-focus-navigation-apple-light.png
  lab-i09-s5-default-apple-light-ja-720.png
  ```

- **Requirement self-check:**  
  1. ○ Apple Dark and Apple Light EN renders exited successfully.  
  2. ○ Kit supplies sections, groups, rows, navigation, tiles, sidebar items, capsules, and progress.  
  3. ○ Reusable card function; both grids have three equal columns and consistent heights. Wrapped titles and subtitles remain separate.  
  4. **partial** Reusable kit progress bar displays 93%; “Manage…” aligns in Dark. Light’s kit group border introduces a one-point inset relative to “See All.”  
  5. ○ Both values precede their navigation chevrons.  
  6. ○ Green status dot and trailing “See All” are visible.  
  7. ○ Commands, PNG paths, and observation limits are reported.

  [C1] Nested cards initially disappeared into their parent surface; fixed with kit-token surfaces and edges.  
  [C2] Narrow Japanese wrapping needed more room; increased every card to 104 points and recaptured at 1040-point page height.  
  [C2] Light-theme trailing alignment differs by one point; retained and reported to preserve kit group geometry. Focus rings stay within their cards/rows, and both themes remain readable.

- **Unclear points:** [Execution] Exact trailing alignment / The kit adds a light-theme group border without a matching section-header inset / General Fix Rule: centralize matching header and group insets in the kit.

- **Discretionary fills:** Used nearest available symbols, category hues, Japanese labels, and a round network-symbol badge. Preserved the reference order.

  Placement suggestions:

  | Setting | Pane | Section |
  |---|---|---|
  | Storage, Manage | iCloud | Storage |
  | Photos, Drive, Passwords, Notes, Messages, Mail | iCloud | Saved to iCloud |
  | Plan, Family, Invitations, Relay, Email, Domain | iCloud | iCloud+ Features |
  | Data Protection, Web Access | iCloud | Data access |

- **Kit gaps:** Reusable destination-card composition, round account badge, and leading status-dot wrapper around the kit section header.

- **Retries:** One visual correction cycle. Three builds succeeded; Cargo lock waits were allowed to finish.

- **Not observed:** Real hover, keyboard focus, VoiceOver, navigation destinations, or action behavior. Interaction captures use forced states. No application was activated.