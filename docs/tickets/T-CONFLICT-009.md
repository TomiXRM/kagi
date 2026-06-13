# T-CONFLICT-009: blame-of-sides(原因 commit 表示)

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

各 conflict file(v0.2 で hunk 単位)に両側の最終 commit sha+summary+author。merge-base からの系列特定

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
