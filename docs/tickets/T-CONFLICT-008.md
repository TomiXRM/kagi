# T-CONFLICT-008: continue / abort の plan 統合

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

continue = バッファ書き出し→stage→操作継続(merge commit / sequencer)。abort = cleanup_state + 開始前復帰。両方 plan→oplog 経由 + バッファ退避

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
