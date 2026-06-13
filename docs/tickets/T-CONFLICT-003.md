# T-CONFLICT-003: Conflict Mode 常設バナー

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

header 直下: op 名 + 進捗 N/M + Continue(全解決まで disabled)/ Abort / Skip(sequencer のみ)。用語は ADR-0058

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
