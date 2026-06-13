# T-BCM-033: dirty working tree 時の warning/stash 提案を実装する

- Status: done
- Group: Integrate
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

checkout/merge/pull/delete/rename-current の plan に dirty 概要 + stash 提案(R6)。既存 plan の dirty 表示を流用

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- merge plan に dirty working tree warning と `git stash push -u` 提案を追加。
- tracking checkout は既存 checkout policy と同様に staged/unstaged を blocker、stash 提案を warning として表示。
