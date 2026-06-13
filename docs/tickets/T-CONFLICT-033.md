# T-CONFLICT-033: LLM 意図要約・解決案(ADR-0061)

- Status: todo(実装開始はユーザー go 後)
- Phase: later
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

later。ResolutionBuffer の提案挿入 API を使用

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
