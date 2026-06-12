# T-COMMIT-002: Commit Preview — staged diff preview

- Status: done
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

## 実装メモ(done)

- staged diff preview は既存配線で完結しており、**新規 diff レンダラは追加していない**(再利用に留めた):
  - Commit Panel の staged 行(flat / tree 両 view)クリック → `select_commit_panel_file(CommitPanelFileRef::Staged{index})`
    → `KagiApp::open_main_diff_wip`(`src/ui/mod.rs`)。
  - `open_main_diff_wip` は `is_staged` 分岐で **`staged_file_diff`**(HEAD tree ↔ index)を呼ぶ。
    unstaged 行は `unstaged_file_diff`。両者を取り違えないことを確認済み(staged 選択 → STAGED diff のみ)。
  - 既存 main-pane diff viewer をそのまま使用:`FileDiffView::from_file_diff` + `highlight_diff_rows`(T-UI-004 syntax highlight)。
    `MainDiffSource::Staged { path }` を source に設定。
- binary / 大ファイル閾値は既存 `patch_to_file_diff` / diff viewer の挙動を踏襲(本チケットで viewer 本体は無改変)。
- `src/ui/file_tree.rs` は staged 側の選択がすでに `Staged` ref を生成するため変更不要。
- 検証: `cargo build` own-code warning 0 / `cargo test` 全 suite green(exit 0)。
