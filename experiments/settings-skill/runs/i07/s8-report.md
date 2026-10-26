- Deliverable / evidence: [examples/lab_i07_s8.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i07_s8.rs). Ran `rustfmt --edition 2021`, `cargo build --example lab_i07_s8 --features gui-e2e`, `target/debug/examples/lab_i07_s8 --list`, and offscreen gallery captures. PNGs: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i07/s8/` (`default`, all seven `pane-*` dark captures, default light, and Japanese 720px default).

- Requirement self-check:
  1. ○ All named pane states rendered in Apple Dark; default rendered in Apple Light.
  2. ○ Every supplied setting is represented once, including custom-theme folder/actions and Analyze Reset….
  3. ○ General contains app-wide appearance/interface basics; Graph, Network & Sync, and Behaviour are separate.
  4. ○ Placement table:

     | Settings | Pane → section | Reason |
     |---|---|---|
     | Theme, Accent, Custom themes/path/actions | General → Appearance | App-wide visual identity |
     | Toolbar, UI Zoom, Compact threshold | General → Interface | App-wide density/presentation |
     | Compact graph, Avatar commit nodes | Graph → Display | Graph-only rendering |
     | Auto-fetch | Network & Sync → Fetching | Remote freshness |
     | Graph Cmd+C, terminal worktree lock | Behaviour → Terminal & clipboard | Interaction/terminal behavior |
     | Markdown opening mode | Behaviour → Documents | Document default behavior |
     | Reduce motion | Behaviour → Motion | Motion preference |
     | Language | Language → Language | Single interface-language choice |
     | Enable, provider, model | Smart Commit → Configuration | One feature configuration |
     | Ignore list, Save, Reset… | Analyze → Ignored paths | One persistent editor task |

  5. ○ Frequent app preferences appear first; every pane uses pane → section only.
  6. ○ Kit controls are reused and sidebar tiles use category hues.
  7. ○ Commands, captures, retry evidence, and unobserved behavior are recorded here.

- Unclear points: none

- Discretionary fills: Used General, Graph, Network & Sync, Behaviour, Language, Smart Commit, Analyze ordering; chose representative static values such as Apple Dark, 100%, and `llama3.2`.

- Kit gaps: One page-local, static multiline Analyze-ignore editor preview; `settings_kit` has no multiline text editor. It uses kit tokens/ring and does not implement editing.

- Retries: 2 — corrected `Role::TextBox` to `Role::TextInput`; then fixed the Japanese 720px Custom Themes action cluster so buttons wrap instead of collapsing the row label.

- Not observed: Real hover/focus, keyboard navigation, VoiceOver, or action/write behavior; this is an offscreen static gallery. The HIG pre-code current/proposed comparison figure was also not created because the task restricted writes to the example file and requested capture outputs only.