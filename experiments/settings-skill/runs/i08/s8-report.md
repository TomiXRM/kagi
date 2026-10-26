- **Deliverable / evidence:** Implemented [examples/lab_i08_s8.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i08_s8.rs), gallery page `lab-i08-s8`. No other source files changed.

  Commands run:

  ```sh
  rustfmt --edition 2021 examples/lab_i08_s8.rs
  cargo build --example lab_i08_s8 --features gui-e2e
  target/debug/examples/lab_i08_s8 --list
  rustfmt --edition 2021 --check examples/lab_i08_s8.rs
  ```

  Captured using:

  ```sh
  KAGI_GALLERY_THEME=<theme> KAGI_GALLERY_LANG=<lang> \
  KAGI_GALLERY_STATE=<state> KAGI_GALLERY_OUT=<png> \
  target/debug/examples/lab_i08_s8
  ```

  Japanese captures additionally used `KAGI_GALLERY_SIZE=720x1000`. Unknown-state validation exited **2** and listed valid states.

  All 26 PNGs are under [target/ui-lab/runs/i08/s8/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i08/s8):

  | State | English PNG filenames | Japanese PNG filename |
  |---|---|---|
  | default | `default-dark.png`, `default-light.png` | `default-dark-ja-720.png` |
  | pane-appearance | `pane-appearance-dark.png`, `pane-appearance-light.png` | Uses identical default pane |
  | pane-graph | `pane-graph-dark.png`, `pane-graph-light.png` | `pane-graph-dark-ja-720.png` |
  | pane-smart-commit | `pane-smart-commit-dark.png`, `pane-smart-commit-light.png` | `pane-smart-commit-dark-ja-720.png` |
  | pane-editor | `pane-editor-dark.png`, `pane-editor-light.png` | `pane-editor-dark-ja-720.png` |
  | pane-terminal | `pane-terminal-dark.png`, `pane-terminal-light.png` | `pane-terminal-dark-ja-720.png` |
  | pane-network | `pane-network-dark.png`, `pane-network-light.png` | `pane-network-dark-ja-720.png` |
  | pane-general | `pane-general-dark.png`, `pane-general-light.png` | `pane-general-dark-ja-720.png` |
  | pane-analyze | `pane-analyze-dark.png`, `pane-analyze-light.png` | `pane-analyze-dark-ja-720.png` |

- **Requirement self-check:**

  | # | Result |
  |---|---|
  | 1 | ○ All nine states rendered successfully in both themes. |
  | 2 | ○ Every specified setting appears once, including all three theme actions and Analyze’s Reset…. |
  | 3 | ○ General contains language only; other settings belong to their domains. |
  | 4 | ○ Placement and pane reasons below. |
  | 5 | ○ Assumed frequency determines pane order; hierarchy is pane → section. |
  | 6 | ○ Kit controls throughout; sidebar tiles use category hues. |
  | 7 | ○ Commands, captures, and observation limits reported. |

  | Setting(s) | Pane → section | Pane reason |
  |---|---|---|
  | Theme; Accent | Appearance → Colours | App presentation belongs together. |
  | Toolbar; UI Zoom; compact layout threshold; Reduce motion | Appearance → Interface | Layout and motion affect presentation. |
  | Folder path; Open theme folder…; Reload Themes; Theme file guide… | Appearance → Custom themes | Theme management stays beside appearance choices. |
  | Compact graph; Avatar commit nodes | Graph → Commit display | Changes commit visualization. |
  | Graph ⌘C copies | Graph → Clipboard | Changes graph interaction. |
  | Enable; provider; model | Smart Commit → Message drafting | Configures generated commit messages. |
  | Markdown opening mode | Editor → Markdown | Changes file viewing. |
  | Worktree lock | Terminal → Worktrees | Controls terminal-related worktree handling. |
  | Auto-fetch | Network → Remote history | Controls background remote synchronization. |
  | Language | General → Language | App-wide basic preference. |
  | Ignore list; Save; Reset… | Analyze → Analyze ignore | Controls analysis input. |

  Opened every PNG. Indicators remain inside their tracks/cells, trailing controls align, and both themes remain readable. No clipping found.

  `[C1]` None beyond the placement-rule conflict documented below.  
  `[C2]` Some offscreen captures contain an additional background highlight on Editor; the selected pane remains uniquely bold. Retained kit-owned behavior.

- **Unclear points:** `[Planning] Single-setting domain panes / §3 discourages them, while S8 explicitly requires domain placement / Follow the scenario-specific requirement; retain Editor, Terminal, and Network panes.`

- **Discretionary fills:** Appearance is first/default. Pane frequency order is an assumption. Theme, language selection, model, folder path, patterns, and switches use fixed fixture values. Existing labels use `Msg`; new section headings use English literals.

- **Kit gaps:** `multiline_patterns(value, selected, forced)` supplies the missing multiline field appearance using kit tokens and `ring`. It displays fixture text without editing or persistence.

- **Retries:** **0 corrective retries.** Waited for Cargo’s shared lock. The initial default capture was repeated during the full matrix.

- **Not observed:** Real hover, keyboard focus/navigation, VoiceOver, text editing, popup menus, persistence, and action execution. Captures used offscreen Metal rendering; no app was activated.