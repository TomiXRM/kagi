# W19-REMOTE-TREE: REMOTE BRANCHES のツリー階層修正(origin/ 一階層問題)

- Status: in-progress
- 担当: worktree agent(Opus)
- 発端: ユーザー指摘 2026-06-13「remote branches の場合 branch 名が origin/ から始まるせいで、
  結局全てが origin/ に対して同じ階層で描かれている」

## 問題

branch list pane(`src/ui/sidebar.rs`)のツリー表示(W13)で、remote branch は名前が
`origin/...` で始まるため、全 branch が `origin` グループ直下のフラットな一覧になる。
remote 名の下で local と同じ prefix グループ化が効いていない。

## スコープ

- REMOTE BRANCHES: **remote 名(origin 等)を第1階層**のグループにし、その下で
  **remote 名を除いた残り**に対して local と同じ first-segment グループ化を適用する
  (例: `origin/feat/x` → origin ▸ feat ▸ x、`origin/main` → origin ▸ main)
- 複数 remote(origin / upstream 等)でそれぞれ独立にグループ化されること
- toggle 開閉状態のキー(`local:feat` 形式)が remote では
  remote 名込みで一意になること(例: `remote:origin:feat`)。既存キーと衝突しない
- クリック(jump)・truncate + name_tooltip・既存挙動は維持
- `src/ui/sidebar.rs` の `group_by_prefix` / `split_first_segment` 周辺。既存 unit test があれば
  追従、なければ純関数部分にテスト追加(`src/ui/sidebar.rs` 内 #[cfg(test)] か tests/)

## 触ってよいファイル

- `src/ui/sidebar.rs` / `src/ui/mod.rs`(collapse キー保持の構造が mod.rs にある場合のみ)
- `docs/tickets/W19-REMOTE-TREE.md`

## 共通規約

- fixture / tempdir のみで検証(ユーザー repo 禁止)。スクリーンショットは PM が確認
- chars() ベース切り詰め、theme() 経由の色、own-code warning 0
- `cargo test` exit code 確認。macOS に timeout なし
- 完了時: 本チケット末尾に実装メモ + Status: done、worktree branch に commit(push/merge しない)
