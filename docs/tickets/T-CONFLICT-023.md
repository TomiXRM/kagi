# T-CONFLICT-023: rename-delete / modify-delete 専用カード

- Status: todo(実装開始はユーザー go 後)
- Phase: v0.2
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

選択肢を文章で提示(keep renamed / keep deleted / 両立案)。全 GUI が弱い差別化点

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
