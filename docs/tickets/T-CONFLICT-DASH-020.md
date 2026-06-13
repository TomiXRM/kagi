# T-CONFLICT-DASH-020: Right Panel を Dashboard に限定する

- Status: done
- Group: Dashboard
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

解決操作(accept/save)を中央へ。右は summary/badge/count/file list/continue/abort/escape のみ

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
dashboard から解決操作は無し(accept/save/both は中央 editor へ集約)。右は summary/badge/count/file list/continue/abort/skip/escape のみ。
