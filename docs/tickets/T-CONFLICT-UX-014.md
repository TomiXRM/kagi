# T-CONFLICT-UX-014: Save resolution で working tree + index へ反映する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

ADR-0068: WT 書き込み + marker 検査 + index unmerged 解消 stage + Resolved へ移動 + oplog

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
`execute_conflict_save(repo, buffer, path)`(conflicts.rs): resolved text を WT 書き込み → marker 残存は**ハードブロック**(以前は warning 止まり)→ `index.add_path`+`index.write` で stage 1/2/3 → stage 0 collapse。`conflict_editor_save`(mod.rs)が repo を開いて実行 → 成功で status=Resolved + re-detect(staged ファイルが conflicted index から外れる)+ oplog(before/after hash) + success toast、失敗(marker)で Refused oplog + error toast。tests: `save_resolution_stages_file_to_stage_zero`(index.has_conflicts() false + stage 0 entry 存在)/ `save_resolution_blocks_on_marker_residue`(marker で拒否 + index 不変)。
