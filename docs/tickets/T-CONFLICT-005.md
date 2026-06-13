# T-CONFLICT-005: ResolutionBuffer backend(自動保存)

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ADR-0057: ファイル別 Result 草稿 + 採用元行 metadata + undo 履歴。~/.kagi/conflicts/ へ debounce 保存・復元。unit test

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
