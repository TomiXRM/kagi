# T-CONFLICT-006: ファイル単位 choose(current/incoming/both-ordered)

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

バッファ上で適用。both は順序選択。binary は choose のみ(専用カード)。zdiff3 で材料取得

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
