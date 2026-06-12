# W19-REMOTE-TREE: REMOTE BRANCHES のツリー階層修正(origin/ 一階層問題)

- Status: done
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

## 実装メモ(W19-REMOTE-TREE)

### 変更内容(`src/ui/sidebar.rs` のみ)

- REMOTE BRANCHES は従来 `group_by_prefix` を `"origin/feat/x"` という
  **フル表示名**に対して掛けていたため、第1階層が常に remote 名(origin)で
  止まり、配下が全部フラットに並んでいた。これを **2階層グループ化**に置換。
- `RemoteBranch` は元々 `remote` と `name`(remote 名を除いた branch 名)を
  分けて保持しているので、それを利用:
  - 第1階層 = `remote`(origin / upstream …、first-seen 順)
  - 第2階層 = 各 remote 内で `name` の最初の `/` セグメント(local と同じ
    `split_first_segment` ロジック)
  - 例: `origin/feat/x` → origin ▸ feat ▸ x、`origin/main` → origin ▸ main
- 新規 pure 関数 `group_remotes()` と enum `RemoteRow<T>`
  (`Remote` / `SubGroup` / `RemoteLeaf` / `SubGroupedLeaf`)を追加。gpui 型を
  含まないので #[cfg(test)] でユニットテスト可能。
- collapse キー:
  - 第1階層 remote header = `remote:origin`(`remote_key`)
  - 第2階層 sub-group = `remote:origin:feat`(`remote_group_key`、remote 名込み
    なので複数 remote 間で `feat` が衝突しない。3セグメントなので2セグメントの
    remote header キーとも、`local:…` キーとも衝突しない)
  - 開閉状態は既存の `branch_groups_collapsed: HashSet<String>`(mod.rs)を
    そのまま流用。mod.rs は変更不要だった。
- 折り畳み伝播: remote header を畳むと配下 sub-group / leaf を全て隠す。
  sub-group を畳むとその leaf のみ隠す。フィルタ有効時は従来通り全展開。
- インデント(`remote_leaf_row` の `depth`): 0=top(12px)/1=remote直下leaf
  (28px)/2=sub-group配下leaf(44px)、sub-group header は 32px。
- クリック jump(`jump_to_commit` / `commit_row_index` 判定)、truncate +
  `name_tooltip`(フル `origin/…` 名)、row id はすべて従来通り維持。
- local branch のレンダパス・`group_by_prefix` は無変更(local は単一階層のまま)。

### テスト(`src/ui/sidebar.rs` の #[cfg(test)])

新規5本追加(全 green):
- `remote_two_levels_basic` — origin/main は直下 leaf、origin/feat/x は feat 配下
- `remote_multiple_remotes_independent` — origin / upstream が独立にグループ化
- `remote_deep_name_keeps_remainder` — origin/feat/ui/x → feat ▸ "ui/x"(単一分割)
- `remote_collapse_keys_unique_and_no_collision` — キーの一意性・非衝突
- `remote_non_ascii_subgroup` — 機能/あ の chars() ベース分割

### 検証

- `cargo build`: 成功、own-code warning 0(既存の block v0.1.6 future-incompat
  のみ、依存クレート由来)
- `cargo test`: 全 suite green。sidebar ユニットテスト 12/12 pass
  (既存7 + 新規5)。fixture/tempdir 以外の検証なし(GUI は PM 確認)。

### 逸脱

なし。mod.rs は触らず(collapse state 構造が既に汎用 `HashSet<String>` で再利用
できたため)、Cargo.toml も無変更。
