# T-BCM-006: disabled reason / tooltip を実装する

- Status: done
- Group: 基盤
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

disabled 項目に理由文字列(commit menu の tooltip と同型)。busy 中は実行系 disabled

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- disabled branch menu items render tooltips with reasons, matching the commit-menu pattern.
- busy state disables mutating items while copy/reveal remain available.
