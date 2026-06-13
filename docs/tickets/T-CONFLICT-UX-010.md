# T-CONFLICT-UX-010: A/B pane header に accept checkbox を移動する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

各 pane header に採用チェック(☑A=current / ☑B=incoming / 両方=both)。GitKraken 風

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
各 pane header に accept toggle(☑/☐ + "accept")。MVP は file-level マッピング(全 hunk に AcceptCurrent/Incoming を適用、解除で Unresolved)。accept_state() が全 hunk choice から集計。
