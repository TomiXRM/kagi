# T-BCM-073: merge/rebase 文言の direction test を作る

- Status: todo
- Group: Tests
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

`Merge <t> into <c>` / `Rebase <c> onto <t>` の label 生成を固定

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)
