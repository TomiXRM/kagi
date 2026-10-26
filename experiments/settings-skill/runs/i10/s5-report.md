- Deliverable / evidence: [examples/lab_i10_s5.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s5.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i10_s5 --features gui-e2e`, `--list`, Apple Dark/Light captures for both states, narrow JA-host capture, and invalid-state validation. PNGs: [dark default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s5/lab-i10-s5-default-dark.png), [light default](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s5/lab-i10-s5-default-light.png), [dark focus](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s5/lab-i10-s5-focus-card-dark.png), [light focus](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s5/lab-i10-s5-focus-card-light.png), [720px capture](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s5/lab-i10-s5-default-light-ja-720x692.png).

- Requirement self-check:
  - ○ Apple Dark and Light EN host renders succeeded.
  - ○ Kit reuse holds for window, sidebar, rows, groups, navigation, icon tiles, capsules, and progress bar.
  - ○ `icloud_card` is reusable; both 3×2 grids have equal columns/heights, and narrow capture shows wrapped titles clear of subtitles.
  - ○ `storage_progress` wraps the kit progress bar; `Manage…` aligns with other trailing actions.
  - ○ Navigation rows render value before chevron.
  - ○ Saved header has green dot and trailing `See All`.
  - ○ Commands, captures, and unobserved behavior are listed below. Capture review found no `[C1]` or `[C2]` issues.

- Unclear points:
  - [Planning] Owner-pinned local Apple HIG corpus unavailable / `~/.claude/skills/apple-design` is absent / install the pinned corpus and obtain the independent HIG review before merge.

- Discretionary fills: Preserved the fixed iCloud pane. Placement: account/storage, saved-data cards, iCloud+ cards, and protection/web navigation all live in iCloud; sidebar uses varied domain hues. Used a blue circular Globe badge as the closest available symbol.

- Kit gaps: Page-local account header/badge, status-section heading, card grid/card, and storage summary layout. All reuse kit tokens/components; the actual progress meter remains `progress_bar`.

- Retries: 1 — invalid-state probe initially used zsh’s read-only `status`; reran successfully without it.

- Not observed: Real pointer hover, real keyboard focus, VoiceOver/accessibility traversal, and localized Japanese copy (scenario strings are English).