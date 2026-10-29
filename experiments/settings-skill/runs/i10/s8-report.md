- Deliverable / evidence: [examples/lab_i10_s8.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i10_s8.rs), gallery page `lab-i10-s8`.

  Commands run:
  ```sh
  rustfmt --edition 2021 examples/lab_i10_s8.rs
  cargo build --example lab_i10_s8 --features gui-e2e
  target/debug/examples/lab_i10_s8 --list
  rustfmt --edition 2021 --check examples/lab_i10_s8.rs
  ```
  Captures used `KAGI_GALLERY_THEME`, `KAGI_GALLERY_STATE`, and `KAGI_GALLERY_OUT`; Japanese captures additionally used `KAGI_GALLERY_LANG=ja KAGI_GALLERY_SIZE=720x860`. Unknown-state rejection was checked. Read-only Pillow inspection measured capsule geometry.

  All 36 PNGs are under [target/ui-lab/runs/i10/s8/](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i10/s8/), with these filename patterns:
  ```text
  lab-i10-s8-{state}-dark.png
  lab-i10-s8-{state}-light.png
  lab-i10-s8-{state}-apple-dark-ja-720.png
  lab-i10-s8-{state}-apple-light-ja-720.png
  ```
  `{state}` is each of `default`, `pane-appearance`, `pane-graph`, `pane-editor`, `pane-smart-commit`, `pane-terminal`, `pane-network-sync`, `pane-general`, and `pane-analyze`.

- Requirement self-check:

  | Item | Result |
  |---|---|
  | 1 | ○ Every pane renders in Apple Dark; all also render in Apple Light. |
  | 2 | ○ Every setting appears once, including the theme path, three theme actions, and Analyze Reset…. |
  | 3 | ○ General contains language only; domain settings have named destinations. |
  | 4 | ○ Placement and rationale are recorded below. |
  | 5 | ○ Appearance leads; hierarchy is pane → section. Frequency order is an assumption. |
  | 6 | ○ Kit components reused; sidebar tiles use category hues. |
  | 7 | ○ Commands, capture paths, and observation limits reported. |

  | Setting | Pane | Section | Pane rationale |
  |---|---|---|---|
  | Theme, Accent | Appearance | Theme | Common presentation adjustments first. |
  | Toolbar, UI Zoom, Compact layout threshold | Appearance | Layout | |
  | Reduce motion | Appearance | Motion | |
  | Theme path, Open…, Reload, Guide… | Appearance | Custom themes | |
  | Compact graph, Avatar nodes | Graph | Commit graph | Commit-history presentation and behavior. |
  | Graph ⌘C copies | Graph | Clipboard | |
  | Markdown opening mode | Editor | Markdown | File-opening behavior belongs to the editor. |
  | Enable, Provider, Model | Smart Commit | Message generation | Keeps generation choices together. |
  | Lock worktree | Terminal | Worktree safety | Safety behavior tied to terminal use. |
  | Auto-fetch | Network & Sync | Background sync | Remote synchronization behavior. |
  | Language | General | Language | App-wide basic without another domain. |
  | Ignore list, Save, Reset… | Analyze | Analyze ignore | Specialized filtering and reset last. |

  Capture critique: no visual `[C1]` or `[C2]` findings remained. Indicators stay inside their controls; trailing edges align; both themes remain readable. Theme capsules each measure **48 physical pixels high**, with identical vertical centers. Japanese actions wrap without clipping.

- Unclear points: `[Planning]` Single-setting pane guidance conflicts with S8’s named-domain requirement / Editor, Terminal, and Network currently have one setting each / Follow the scenario-specific domain placement requirement.

- Discretionary fills: Chose the pane order, English section titles where needed, a 1000×860 default size, and fixture values for the theme folder, model, and ignore patterns. Used existing `Msg` strings where available.

- Kit gaps: `multiline_value(value, forced)` renders the full-width ignore-text fixture using kit geometry and ink. It does not implement editing.

- Retries: **1** — corrected the multiline component’s return type from `Div` to `Stateful<Div>`. Rebuild passed.

- Not observed: Real hover, keyboard focus, VoiceOver, live text editing, popup menus, provider/model detection, persistence, or action execution. Captures were offscreen; no application was activated.