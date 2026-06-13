# T-CONFLICT-020: 3-pane + 編集可能 Result view

- Status: todo(実装開始はユーザー go 後)
- Phase: v0.2
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ADR-0059。uniform_list 仮想化、同期スクロール。退路 = inline marker 編集モード

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証
