# T-BCM-004: MenuGroup / MenuItem model を定義する

- Status: done
- Group: 基盤
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

既存 MenuItem を再利用。branch 用 action enum BranchAction を branch_menu.rs に追加

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- `MenuItem` / `MenuGroup` in `src/ui/context_menu.rs` are now generic with `CommitAction` as the default.
- `BranchAction` lives in `src/ui/branch_menu.rs` and reuses the same item/group model.
