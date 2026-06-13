# T-CONFLICT-UI-002: A/B/Result pane の border/background/label を改善する

- Status: done
- Group: Layout
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

各 pane に border + editor bg を周囲より暗く + pane title + 選択 hunk highlight

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
pane ごとに border + editor bg(bg_base、周囲 surface より暗め)+ title。選択 hunk highlight は MVP では accept-toggle のチェック状態 + both-strip active 表示で代替(InputState 行ハイライト API は v0.2、POLISH-043 参照)。
