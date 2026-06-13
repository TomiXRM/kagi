# T-BCM-014: Create branch from here を実装する

- Status: done
- Group: Safe
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

既存 create-branch modal を選択 branch tip 起点で開く(commit menu の Create branch here と同 handler)

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- `Create branch from here` dispatches to the existing `open_create_branch_modal` with the selected branch tip commit.
- the handler is shared with the commit context menu path.
