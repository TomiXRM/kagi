# W13-BRANCHTREE: branch list の `/` 区切りツリー表示(ユーザー要望)

- Status: done
- 担当: worktree agent(Opus)

## 背景

ユーザー要望: sidebar の LOCAL BRANCHES で `feat/xxx` `fix/yyy` のような `/` 区切りの
ブランチ名をツリー構造(プレフィックスでグループ化)で表示し、グループを折りたたみ
(toggle)できるようにしたい。

## スコープ

1. **グルーピング**: LOCAL BRANCHES(と REMOTE BRANCHES も同様に)を `/` の最初のセグメントで
   グループ化。例: `feat/a`, `feat/b`, `fix/c`, `main` →
   ```
   ▾ feat (2)
       a
       b
   ▾ fix (1)
       c
   main
   ```
   - 多段ネスト(`feat/ui/x`)は file_tree.rs と同じ「単一子チェーンの圧縮」で1階層目のみで可
     (深いネストの完全ツリーは過剰。`feat/ui` グループ → `x` で十分なら多段でもよい — 実装判断を
     メモに残す)
   - グループに属さない名前(`main` 等)はトップレベルにそのまま
2. **折りたたみ**: グループ行クリックで toggle(▸/▾ + 件数)。状態は
   `sidebar_collapsed` と同様に KagiApp に保持し reload を跨いで維持
   (キーは `local:feat` / `remote:origin/feat` 形式で衝突回避)
3. **既存挙動の維持**: 葉(ブランチ行)の click=jump / dblclick=checkout / ✕ delete /
   upstream ↑↓ 表示 / current ✓ / filter(フィルタ中はマッチした葉のグループを自動展開)/
   truncate+tooltip — すべて回帰なし
4. **remote**: `origin/...` は remote 名でまず1段(既存表示が `origin/feat/x` なら
   `origin` グループ → `feat` グループ or `feat/x` 葉。実装判断をメモに)
5. headless: `[kagi] sidebar:` ログは既存互換(グループ数は出さなくてよい)

## 完了条件

- [x] `/` 区切りブランチがグループ化され toggle できる(PM スクリーンショット)
- [x] グループ折りたたみ状態が reload(checkout 等)後も維持
- [x] filter 入力でマッチ葉が見える(グループ自動展開)
- [x] 既存の click/dblclick/✕/✓/↑↓/tooltip 回帰なし
- [x] 既存 headless ログ回帰なし / `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/sidebar.rs`(主戦場)/ `src/ui/mod.rs`(state 追加の最小限)
- `docs/tickets/W13-BRANCHTREE.md`

## 触ってはいけないファイル

- `src/git/` / `vendor/` / `tests/*` / `scripts/*` / `Cargo.toml`

## テスト方法

1. `cargo test`
2. fixture に `feat/a` `feat/b` `fix/c` 等を追加して headless + PM スクリーンショット
3. 検証は fixture / tempdir のみ

## リスク

- 色は theme() 経由(const 直書き禁止)。文字列処理は chars() ベース
- グループ行と葉行の click 競合(toggle と jump の取り違え)に注意
- force 系コード追加禁止(全体規約)

## 実装メモ

### グルーピング仕様の最終判断

- **単一第1セグメント split**(多段ネストの完全ツリーは作らない)。`feat/a` `feat/b`
  → グループ `feat` (2) + 葉 `a` `b`。`main` のような `/` を含まない名前はトップレベル葉。
  深いネスト `feat/ui/x` は **グループ `feat` + 葉 `ui/x`**(残り全体を1葉として表示)。
  チケット「1階層目のみで可」を採用。file_tree.rs の多段圧縮(`a/b/c`)とは別方針で、
  branch は階層が浅く click モデルが単純な方がユーザー操作が読みやすいため。
- **remote**: 表示名 `origin/feat/x` の第1セグメント=remote 名でグループ化。
  → グループ `origin` + 葉 `feat/x`。チケット「remote 名でまず1段」を採用。
- **空セグメントはトップレベル**: `/x` や `feat/` は split しない(prefix/rest どちらかが空なら None)。
- 文字列処理はすべて `chars()` ベース(`split_first_segment`、byte slice なし)で非 ASCII 安全。
- グループ/葉の順序は入力順を保持(グループは初出順、グループ内葉も入力順)。

### toggle 状態 / reload 跨ぎ維持

- グループは動的名のため `sidebar_collapsed: HashSet<&'static str>` とは別に
  **`branch_groups_collapsed: HashSet<String>`** を KagiApp に新設。
- キーは `local:feat` / `remote:origin` 形式(`group_key(section, prefix)`)。衝突回避済み。
- セマンティクスは `sidebar_collapsed` と同じ **default-expanded**(キーがあれば collapsed)。
  チケット例の `▾`(展開)初期表示に一致。
- reload(checkout 等)で reset しないため(コンストラクタ初期化のみ・apply_tab_view で触らない)
  折りたたみ状態は自動的に維持される。

### filter 自動展開

- `has_filter` のとき各グループの `group_collapsed` を強制 false にして、
  マッチした葉(`*_filtered` で既に絞り込み済み)が必ず見えるようにした。
  collapse 状態自体は破壊しない(filter を消すと元の ▸/▾ に戻る)。

### 既存葉挙動の維持

- 葉描画を `local_leaf_row` / `remote_leaf_row` クロージャに抽出し、グループ葉・トップ葉で共有。
- click=jump / dblclick=checkout(open_plan_modal)/ ✕ delete / ✓ current /
  ↑↓ upstream / truncate+tooltip / row id(`sidebar-branch-{full}` 等)は **すべて full
  branch name 基準**。表示テキスト(`display_label`)だけが prefix-stripped。挙動回帰なし。
- グループ葉は左 padding を増やして(`pl(28px)`)子として読めるように。グループ行は `pl(20px)`。
- 色はすべて theme() 経由(グループ行は `theme().text_sub`)。const 直書きなし。

### mod.rs 変更の全列挙

1. 構造体に `pub branch_groups_collapsed: HashSet<String>` フィールド追加(`sidebar_collapsed` の直後)。
2. コンストラクタ 2 箇所に `branch_groups_collapsed: HashSet::new()` 初期化追加。
3. reload preservation コメントに W13 注記追記(実コードの reset は無し=自動維持)。
4. render: `let branch_groups_collapsed = self.branch_groups_collapsed.clone();` 追加。
5. `render_body` 呼び出しに引数追加 / `render_body` シグネチャに
   `branch_groups_collapsed: HashSet<String>` 追加 / `render_sidebar` 呼び出しに
   `&branch_groups_collapsed` 追加。

### sidebar.rs 変更

- 純関数追加: `GroupRow<T>` enum / `group_by_prefix` / `split_first_segment` / `group_key`。
- `render_sidebar` シグネチャに `groups_collapsed: &HashSet<String>` 追加。
- LOCAL / REMOTE セクションの葉ループをグループ化レンダリングに置換。
- unit test 7 件追加(split・grouping・remote・key・top-level)。

### テスト / headless

- `cargo test` 全パス(lib 19 + main 74〔うち W13 新規 7〕+ 統合多数、failed 0)。
- own-code warning 0(`cargo build` クリーン)。
- fixture(`feat/a` `feat/b` `fix/c` `feat/ui/x` 追加)で headless 起動:
  `[kagi] sidebar: local=7 remote=3 tags=1 stashes=1 worktrees=1 filter=""` —
  既存ログ書式完全互換、panic なし。
