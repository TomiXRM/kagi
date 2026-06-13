# T-CONFLICT-004: conflict file list + 進捗表示

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

sidebar or panel に unresolved/resolved/needs-review。prev/next 未解決ナビ(KDiff3 流)

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
