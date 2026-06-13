# T-BCM-060: remote branch context menu を実装する

- Status: done
- Group: Remote
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

R4 の項目構成。kind=Remote の BranchMenuContext 出し分け

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- remote branch の `Checkout as local branch` と `Merge remote into current` を enabled 化。
- delete remote / fetch remote / PR / rebase は MVP 外または既存 stub のまま維持。
