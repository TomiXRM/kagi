# W13-BRANCHTREE: branch list の `/` 区切りツリー表示(ユーザー要望)

- Status: in-progress
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

- [ ] `/` 区切りブランチがグループ化され toggle できる(PM スクリーンショット)
- [ ] グループ折りたたみ状態が reload(checkout 等)後も維持
- [ ] filter 入力でマッチ葉が見える(グループ自動展開)
- [ ] 既存の click/dblclick/✕/✓/↑↓/tooltip 回帰なし
- [ ] 既存 headless ログ回帰なし / `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

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
