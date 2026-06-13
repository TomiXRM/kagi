# T-CONFLICT-022: undo / redo(バッファ履歴)

- Status: todo(実装開始はユーザー go 後)
- Phase: v0.2
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ファイル単位の操作履歴。出所可視化(行ごと採用元の色分け)

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
