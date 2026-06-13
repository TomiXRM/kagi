# T-CONFLICT-024: 外部 merge tool / terminal 連携

- Status: todo(実装開始はユーザー go 後)
- Phase: v0.2
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ADR-0060。settings の mergetool 起動 + watcher 取り込み

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
