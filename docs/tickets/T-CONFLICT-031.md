# T-CONFLICT-031: 作業順序の提案(機械的に解けるもの先頭)

- Status: todo(実装開始はユーザー go 後)
- Phase: v1.0
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ソート規則の設計 + 実装

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
