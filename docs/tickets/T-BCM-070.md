# T-BCM-070: local/current/upstream なし/あり の availability test を作る

- Status: done
- Group: Tests
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

branch_context_menu_items の表を unit test で固定

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- added `#[cfg(test)]` tests in `src/ui/branch_menu.rs` for local upstream/no-upstream, current branch, remote branch, and busy availability.
- tests are pure and do not use user repositories.
