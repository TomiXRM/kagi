# T-CONFLICT-001: ConflictSession 検出 backend(state + index conflicts)

- Status: todo(実装開始はユーザー go 後)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

`src/git/conflicts.rs`(新規): Repository::state() + Index::conflicts() から ConflictSession を構築(op 種別、files、kind 分類)。unit test(fixture で merge/cherry-pick conflict 再現)

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
