# T-COMMIT-002: Commit Preview — staged diff preview

- Status: todo
- 依存: T-COMMIT-001 / 既存 diff viewer(T012 / T-UI-003 / T-UI-004)
- 関連: ADR-0039、lane W14-PREVIEW

## 背景

commit 直前に **staged の diff** を確認できるようにする(unstaged は含めない)。既存の staged_file_diff /
diff viewer を Commit Panel の preview として束ねる。

## スコープ

- Commit Panel から、staged な各ファイルの **staged diff**(`staged_file_diff`)を選んで preview 表示。
- unstaged diff は混ぜない(「commit に入るもの」だけ見せる)。
- 既存 diff viewer(syntax highlight 含む T-UI-004)を再利用。新規 diff レンダラは作らない。

## 完了条件

- [ ] staged ファイル選択で staged diff が表示される(unstaged を表示しない)
- [ ] binary / 大ファイルで固まらない(既存 viewer の閾値を踏襲)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs` / `src/ui/file_tree.rs`(staged 側の選択配線)
- `docs/tickets/T-COMMIT-002.md`

## 触ってはいけないファイル

- 上記以外(diff viewer 本体のロジック改変は避け、再利用に留める)

## テスト方法

1. `cargo test`
2. fixture / tempdir のみ
3. UI は PM がスクリーンショット確認

## リスク・規約

- staged と unstaged の diff source を取り違えない(`staged_file_diff` を使う)
