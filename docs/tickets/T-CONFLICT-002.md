# T-CONFLICT-002: RepoMode::Conflict と外部起因検出

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

KagiApp に RepoMode。起動時 + watcher で検出して Mode 出入り(CLI 起因含む)。headless ログ `[kagi] conflict-mode: <op> N file(s)`

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
