- Deliverable / evidence: [examples/lab_i07_s3.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s3.rs), gallery page `lab-i07-s3`. Only this source file was written.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i07_s3.rs
  rustfmt --edition 2021 --check examples/lab_i07_s3.rs
  cargo build --example lab_i07_s3 --features gui-e2e
  cargo build --example lab_i07_s3 --features gui-e2e --message-format=json
  target/debug/examples/lab_i07_s3 --list
  ```
  Captures used `KAGI_GALLERY_THEME`, `KAGI_GALLERY_LANG`, `KAGI_GALLERY_STATE`, and `KAGI_GALLERY_OUT`; narrow captures additionally used `KAGI_GALLERY_SIZE=720x980`. After detecting shared-executable replacement, captures used the verified `gui-e2e` artifact `target/debug/examples/lab_i07_s3-033f7b009dca4054`. Invalid-state rejection was also verified.

  All eight PNGs were opened and inspected under [target/ui-lab/runs/i07/s3/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s3):
  ```text
  lab-i07-s3-default-dark-en.png
  lab-i07-s3-default-light-en.png
  lab-i07-s3-default-dark-ja.png
  lab-i07-s3-default-light-ja.png
  lab-i07-s3-default-dark-ja-720.png
  lab-i07-s3-default-light-ja-720.png
  lab-i07-s3-focus-switch-apple-dark-en.png
  lab-i07-s3-focus-switch-apple-light-en.png
  ```

- Requirement self-check:
  1. ○ Apple Dark EN, Apple Light JA, and 720 pt JA renders succeeded.
  2. ○ Every control, row, group, section, sidebar item, and tile uses the kit.
  3. ○ All titles, labels, and descriptions use `Msg`; only specified static values are literal.
  4. ○ Japanese descriptions wrap without clipping, overlap, or escaping controls at 720×980.
  5. ○ Four category-colored sidebar destinations appear; General alone is selected.
  6. ○ Commands, evidence paths, and observation limits are reported.

  Capture critique: indicators match values; sibling controls retain kit geometry; trailing controls align; text segments and category hues follow the skill; light and dark remain legible. No `[C2]` findings or visual corrections were needed. The placement conflict below remains `[C1]`.

- Unclear points:
  - `[Planning]` `[C1]` Domain-pane placement conflicts with S3 / §3 requires separate panes, while S3 requires three sections inside General / Follow S3’s explicit layout here; reconcile these requirements in future briefs.
  - `[Execution]` Shared executable changed to a non-offscreen build / Concurrent compilation replaced the unqualified executable / Capture using the feature-specific artifact.

- Discretionary fills: retained the baseline’s Theme accent and Icon Only selections; added `focus-switch`, changing only the compact switch’s forced appearance; used 980 pt height to show every row.

  | Setting | Pane | Section |
  |---|---|---|
  | Theme, Accent, Toolbar, UI Zoom, Compact threshold | General | Appearance |
  | Compact graph, Avatar commit nodes | General | Graph |
  | Auto-fetch | General | Network |

- Kit gaps: none; no page-local controls.

- Retries: two failed capture attempts, both recovered after one feature rebuild and switching to the feature-specific executable. No layout revisions.

- Not observed: real hover, keyboard focus/navigation, VoiceOver, or 720×692 containment. Focus evidence is forced rendering. No tests, Clippy, global formatting, or app activation was performed.