- Deliverable / evidence: [examples/lab_i09_s8.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i09_s8.rs). Only this source file was added; concurrent worktree changes were untouched. Commands run: `rustfmt --edition 2021 examples/lab_i09_s8.rs`; `cargo build --example lab_i09_s8 --features gui-e2e`; `target/debug/examples/lab_i09_s8 --list`; offscreen gallery renders for every state/theme; `git diff --check -- examples/lab_i09_s8.rs`. PNGs: `target/ui-lab/runs/i09/s8/lab-i09-s8-{default,pane-appearance,pane-graph,pane-editor,pane-network,pane-terminal,pane-smart-commit,pane-analyze,pane-language}-dark.png`, plus `lab-i09-s8-default-light.png` and `lab-i09-s8-default-ja-720-dark.png`.

- Requirement self-check:
  1. ○ All nine pane states rendered successfully in Apple Dark; default also rendered in Apple Light.
  2. ○ Every listed setting appears once, including Custom themes folder/actions and Analyze Save/Reset.
  3. ○ No grab-bag Behaviour pane: Graph copy is in Graph, Markdown mode in Editor, terminal locking in Terminal, and network settings in Network.
  4. ○ Placement table:

    | Setting | Pane | Section |
    | --- | --- | --- |
    | Theme, Accent, Toolbar, UI Zoom, Compact threshold, Reduce motion | Appearance | Appearance |
    | Custom-theme folder, Open theme folder…, Reload Themes, Theme file guide… | Appearance | Custom themes |
    | Compact graph, Avatar commit nodes, Graph Cmd+C copies | Graph | Graph |
    | Markdown opening mode | Editor | Editor |
    | Auto-fetch | Network | Network |
    | Lock worktree while terminal is open | Terminal | Terminal |
    | Smart Commit enable, provider, model | Smart Commit | Smart Commit |
    | Analyze ignore list, Save, Reset… | Analyze | Analyze ignore |
    | Language | Language | Language |

    Pane reasons: Appearance collects visual/display choices; Graph owns commit-graph behavior; Editor owns file-opening behavior; Network owns remote refresh; Terminal owns terminal-side worktree protection; Smart Commit owns LLM configuration; Analyze owns analysis inputs; Language is the app-wide localization preference.
  5. ○ Pane order is Appearance → Graph → Editor → Network → Terminal → Smart Commit → Analyze → Language; each pane has one section level.
  6. ○ Settings-kit owns rows, groups, sidebar items, tiles, switches, segments, pop-ups, steppers, and buttons. Sidebar tiles use category hues. Capture critique [C1]: moved Graph copy and Markdown mode out of Terminal; [C2]: none.
  7. ○ Commands, paths, and unobserved interaction modes are listed here.

- Unclear points: none

- Discretionary fills: Added an Editor pane to keep Markdown behavior with its affected object; placed Reduce motion under Appearance; added ellipses to actions that open an external folder/guide.

- Kit gaps: `multiline_patterns`, a display-only multiline Analyze ignore field using kit metrics, ink, and focus-ring tokens.

- Retries: 2 — one build retry after owning the multiline text value; one capture/build cycle after correcting the information architecture.

- Not observed: Real pointer hover, keyboard focus traversal, editable text input, VoiceOver, or actual action execution; gallery captures use static/rest states only.