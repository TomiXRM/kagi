# W6-TABSPEED: tab 切替の高速化(キャッシュ + 非同期読込 + ローディング表示)

- Status: in-progress
- 担当: worktree agent(Opus)
- 関連 ADR: 0030 / 0027(tabs)

## 背景

ユーザー報告: 「タブの切り替えにもっさりとした重さを感じた」。
switch_repo が UI スレッドで snapshot を同期実行しているため。方針は ADR-0030(両取り):
キャッシュ済みは即時 swap、未キャッシュはローディング表示 + background 読込。

## スコープ

1. **TabViewState 抽出**: snapshot 由来の純データ(rows / details / branches /
   remote_branches / tags / stashes / branch_upstream_info / commit_row_index /
   branch_targets / status_summary / toolbar_state / header / repo 名 等)を struct に括り出し、
   `build_tab_view(snapshot, repo_name) -> TabViewState`(純関数、Send)と
   `KagiApp::apply_tab_view(state)`(main スレッド、代入のみ)に分離。
   既存 from_snapshot / reload はこの2つの合成として書き直す(挙動・ログ完全互換)
2. **background 読込**: `load_repo_async(path, cx)` = cx.background_spawn で
   snapshot + build_tab_view → main で apply + tab_cache 更新。
   `switch_generation: u64` で連打を防御(最後の switch のみ適用)
3. **キャッシュ**: `tab_cache: HashMap<PathBuf, TabViewState>`。
   switch 時: キャッシュあり → 即 apply(体感ゼロ)→ background revalidate → 完了時に再 apply。
   キャッシュなし → ローディング表示 → background 読込 → apply。
   close_tab で evict。reload() 完了時もキャッシュ更新
4. **ローディング表示**: main pane 中央に「Loading <repo名>…」+ FooterStatus::Busy。
   tab strip は操作可能のまま。per-repo UI 状態リセット(selection 等)は従来どおり
5. **headless 互換**: 既存ログの出力順を維持するため、headless 経路(KAGI_* / 起動時の初期 tab)は
   従来どおり**同期読込**でよい(main.rs は変更最小)。新ログ:
   `[kagi] tab-switch: <name> cached=yes|no` と background 完了時 `[kagi] tab-load: <name> rows=N`
6. (余力があれば)watcher の reload_external も同経路化(ADR-0030 §6)。無理なら触らない

## 完了条件

- [ ] キャッシュ済み tab への切替が即時(PM が大きめ repo で体感確認)
- [ ] 初回 open はローディング表示が出て UI が固まらない
- [ ] 切替連打で最終 tab の内容が正しく表示される(generation ガード)
- [ ] 切替後の watcher / terminal / context menu / commit panel が従来どおり動作
- [ ] 既存 headless ログ(起動・KAGI_OPEN_REPO 含む)に回帰なし + 新ログ追加
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/tabs.rs` / `src/ui/mod.rs`(TabViewState 抽出を含む)/ `src/main.rs`(最小限)
- `docs/tickets/W6-TABSPEED.md`

## 触ってはいけないファイル

- `src/git/`(読み取り専用 — snapshot 関数のシグネチャ変更禁止)/ `tests/*` / `scripts/*` / `Cargo.toml`

## テスト方法

1. `cargo test`(exit code 直接確認)
2. fixture 2つで headless(既存全回帰 + 新ログ)
3. 大きめ repo の体感は PM が確認

## リスク

- **並行 lane 注意**: codex 4 lane(cm-create/apply/checkout/compare)が src/ui/mod.rs /
  ops.rs を編集中。mod.rs の変更は from_snapshot/reload の分離部分に限定し、
  **変更点を完了報告で全列挙**(PM が merge)
- from_snapshot の分離で初期化漏れ(フィールド追加が多い struct)— コンパイルエラー駆動で潰す
- 古い generation の結果が UI を巻き戻す事故 — apply 前に generation 検査を必ず行う
- 文字列切り詰めは chars() ベース / force 系コード追加禁止(全体規約)

## 実装メモ(worktree agent / Opus)

### 構造
- `TabViewState`(src/ui/mod.rs)= snapshot 由来の純データ一式。`#[derive(Clone)]`、
  Send 型のみ(SharedString / Vec / HashMap / プレーン値)。Entity / FocusHandle /
  UniformListScrollHandle は一切含めない。フィールド:
  `header, rows, details, branches, stashes, is_dirty, branch_targets,
   commit_row_index, status_summary, toolbar_state, remote_branches, tags,
   branch_upstream_info`(計 13)。
- `build_tab_view(snap: &RepoSnapshot, repo_name: &str) -> TabViewState`(free fn、
  Send)= 旧 `from_snapshot` のインライン計算と**全 eprintln! ログ**(graph /
  commit list / sidebar / statusbar / toolbar)をそのまま移設した純関数。
- `KagiApp::apply_tab_view(view)` = view データの代入のみ(transient UI は触らない)。
- `from_snapshot` は `build_tab_view` + oplog ロード + 構造体合成に書き直し(挙動・ログ完全互換)。
- `reload` は `build_tab_view` → transient リセット → `tab_cache` 更新(ADR-0030 §5)→
  `apply_tab_view` に書き直し(ログ・挙動互換)。

### async + cache + generation
- `switch_repo`(tabs.rs)= 同期 reload を廃止。`reset_per_repo_ui()` で UI リセット →
  `switch_generation` を bump → cache hit なら即 `apply_tab_view`(体感ゼロ)、miss なら
  `loading_tab = Loading <name>…` + `FooterStatus::Busy` → `arm_watcher` + notify →
  `load_repo_async`。
- `load_repo_async`(tabs.rs)= `cx.background_spawn` で snapshot+build(W3 パターン)、
  main 側で `switch_generation == generation` を検査してから apply + cache 更新 +
  `loading_tab=None` + Busy footer を Idle に戻す。
- `close_tab` は `tab_cache.remove(&closed.path)` で evict。
- ローディング表示: `render_loading_placeholder`(center pane のみ差し替え。tab strip は操作可能)。

### 新ログ(GUI switch 経路のみ)
- `[kagi] tab-switch: <name> cached=yes|no`
- `[kagi] tab-load: <name> rows=N`(background 完了時)
- headless 経路(KAGI_OPEN_REPO / 起動初期 tab)は init_tab→reload で**従来どおり同期**のため
  これらのログは出ず、既存ログ順は完全に回帰なし(fixture 2つで確認済み)。

### 検証
- `cargo test` 全パス(exit 0)、own-code warning 0。
- fixture ×2 headless: 単一 repo / KAGI_OPEN_REPO 二タブ / KAGI_SELECT_FIRST+JUMP で
  既存ログ全回帰なし。GUI switch の体感は PM 確認待ち。
- watcher の reload_external は同経路化せず(stretch、未着手)。
