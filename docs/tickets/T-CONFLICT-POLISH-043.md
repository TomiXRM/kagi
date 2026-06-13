# T-CONFLICT-POLISH-043: selected hunk highlight を改善する

- Status: done
- Group: Polish
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

選択中 hunk の背景色を明確に

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
選択 hunk highlight 改善は MVP では accept-toggle チェック状態 + both active 表示で代替。InputState の行レベルハイライト overlay は v0.2 deferred。
