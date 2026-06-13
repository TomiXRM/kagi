# T-BCM-042: worktree 作成 operation plan を実装する

- Status: done
- Group: Worktree
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

既存 plan_create_worktree/start_create_worktree を menu から起動

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- `plan_open_worktree_for_branch` / `execute_open_worktree_for_branch` を追加し、既存 create-worktree modal/start path から呼び分け。
- 新規 branch 作成の既存 `plan_create_worktree` / `execute_create_worktree` の挙動は維持。
