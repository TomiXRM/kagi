# T-CONFLICT-030: rebase 多 step 進捗 UX

- Status: todo(実装開始はユーザー go 後)
- Phase: v1.0
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

commit 2/5 表示、step ごとの skip、ORIG_HEAD 復帰

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
