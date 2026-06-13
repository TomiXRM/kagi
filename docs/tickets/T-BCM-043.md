# T-BCM-043: branch が別 worktree で checkout 済みの場合の warning を実装する

- Status: done
- Group: Worktree
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

checkout=blocker(ADR-0051)/ Open worktree=既存 path 案内(ADR-0054)。判定は BranchMenuContext へ

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- snapshot の `Worktree` に checked-out branch 名を追加し、BranchMenuContext に別 worktree path を渡す。
- checkout は別 worktree checkout 済み branch を disabled、Open worktree は既存 path を footer/toast で案内。
