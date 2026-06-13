# T-CONFLICT-FLOW-030: merge Continue を commit message 画面遷移に変更する

- Status: done
- Group: Flow
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

ADR-0068: 全 resolved 後、即 commit せず commit message panel(merge message)へ

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done, MVP)
`plan_conflict_continue_route`(conflicts.rs)が gate 通過後 merge を `ContinueRoute::MergeCommitPanel{message}` に分岐。`conflict_continue(window,cx)`(mod.rs)が `open_commit_panel` → input/panel に merge message プリフィル → `conflict_merge_commit_pending=true`。render は pending 時 conflict body を隠し通常 body(commit panel)を表示(MERGE_HEAD は保持)。merge message は MERGE_MSG(コメント除去)優先、無ければ "Merge <incoming> into <current>"(ADR-0058 role 名)。即 commit しない(test `merge_continue_routes_to_commit_panel_without_committing`: HEAD 不変 + MERGE_HEAD 残存)。
