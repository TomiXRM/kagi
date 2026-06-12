# ADR-0016: Changed Files Tree/List and Diff Viewer

- Status: Accepted / Date: 2026-06-12(T018/T-UI-002/003/004 の決定を統合追認)

## Decision
- Changed Files は flat(Path)⇄ Tree をトグル可能(Inspector 側にも導入。Commit Panel は導入済み)。両ビューで status バッジ・件数・選択ハイライト・active ファイル強調を共通仕様とする
- Diff は main pane 全幅(T-UI-003)・行番号・tree-sitter ハイライト(T-UI-004)・binary/renamed/deleted の明示表示
- **大 diff**: 行数 > 2000 で hunk 単位の fold(初期は先頭 N hunk 展開 + 「Show more」)。それ以下は全展開
- commit diff(HEAD↔parent)と WIP diff(index↔WT / HEAD↔index)は MainDiffSource で区別(導入済み)

## Consequences
- fold 状態は MainDiffView 内の一時状態(永続化しない)
