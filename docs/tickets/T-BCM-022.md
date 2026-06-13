# T-BCM-022: Push and set upstream operation plan を実装する

- Status: done
- Group: Sync
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

upstream 未設定 branch 用。plan に作成される upstream 名を表示。execute は既存 push 経路 + branch.<name> config 設定

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ

- `Push and create upstream` now opens a plan showing the target `remote/branch`.
- Execute uses `git push -u <remote> <branch>` through the shared CLI wrapper; no force flags.
