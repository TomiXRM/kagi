# T-BCM-020: Pull item に behind count を表示する

- Status: done
- Group: Sync
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

`Pull ↓3`。behind=0 は no-op 表示で disabled。upstream なしは disabled(ADR-0050)

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ

- `Pull ↓N` / `Pull (up to date)` labels are live in `branch_context_menu_items`.
- Non-current local branch pull uses `plan_pull_branch_ff` / `execute_pull_branch_ff` for fetch + fast-forward-only ref update with no working-tree change.
