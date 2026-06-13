# T-BCM-001: Branch item の right-click event を取得する

- Status: done
- Group: 基盤
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

sidebar.rs の branch 行(local/remote、folder 行は除外)に on_mouse_down(Right)。anchor 位置を保存

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## Implementation memo

- local/remote branch leaf rows in `src/ui/sidebar.rs` now handle `on_mouse_down(Right)` and open the branch context menu at the cursor.
- folder/group rows remain no-op for this lane.
