# T-CONFLICT-032: rerere 相当の解決再利用

- Status: todo(実装開始はユーザー go 後)
- Phase: v1.0
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

設計 ADR から(libgit2 非対応のため自前)

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
