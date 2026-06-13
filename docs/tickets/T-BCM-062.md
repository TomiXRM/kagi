# T-BCM-062: Merge remote branch into current branch operation plan を実装する

- Status: done
- Group: Remote
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

remote-tracking ref を target に ADR-0052 の merge plan を流用

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- remote-tracking ref 名をそのまま `plan_merge_branch` の target に渡し、local branch merge と同じ in-memory plan/execute path を共有。
- remote branch menu の Merge は `Merge origin/x into <current>` として表示される。
