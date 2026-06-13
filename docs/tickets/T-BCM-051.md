# T-BCM-051: branch 名 validation を実装する

- Status: done
- Group: Manage
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

git2 の参照名検証(Branch::rename のエラー写像)+ 既存名衝突チェック。純関数 + unit test

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ

- Added pure `validate_branch_rename(old, new, existing)` with collision, empty, whitespace, same-name, and libgit2 refname checks.
- Unit coverage is in `tests/branch_sync_test.rs`.
