# T-BCM-002: 右クリック branch を selection state に反映する

- Status: done
- Group: 基盤
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

menu を開く前に対象 branch を選択状態へ(jump はしない)。既存 select と冪等ガード整合

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- branch-menu open paths call the existing `jump_to_branch` / `jump_to_commit` selection paths before showing the menu.
- those paths already guard repeated selection so `select()` does not toggle an already-selected row off.
