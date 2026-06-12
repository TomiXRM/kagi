# ADR-0030: 非同期リポジトリ読込と tab キャッシュ(stale-while-revalidate)

- Status: Accepted / Date: 2026-06-12

## 背景

tab 切替(W4-TABS)は `switch_repo` が UI スレッドで snapshot(最大 10k commits の
topo walk + status + refs + graph layout + row 構築)を同期実行するため、
大きい repo で「もっさり」する(ユーザー報告)。

## Decision

**キャッシュとローディング表示の両方**を採用する:

1. **stale-while-revalidate キャッシュ**: 
   ```rust
   // tab 横断で保持(close_tab で evict)
   tab_cache: HashMap<PathBuf, TabViewState>
   // TabViewState = snapshot 由来の純データ一式(rows / details / branches /
   //   remote_branches / tags / stashes / branch_upstream_info / status_summary /
   //   toolbar_state / header 等。Entity / handle 類は含めない)
   ```
   切替時にキャッシュがあれば**即時表示**(swap のみ、フレーム内)→ 直後にバックグラウンドで
   再 snapshot して鮮度を回復(完了時に差し替え + キャッシュ更新)
2. **バックグラウンド snapshot**: `RepoSnapshot` は純データ(Send)なので
   `cx.background_spawn` で構築できる(W3 の pull_blocking と同パターン)。
   `TabViewState` の構築(rows/details/graph layout)も純データ処理なので background 側で行う
3. **ローディング表示**: キャッシュが無い場合(初回 open)は main pane 中央に
   `Loading <repo>… ⟳` プレースホルダ + FooterStatus::Busy。sidebar/inspector は空表示。
   tab strip は操作可能のまま
4. **世代ガード**: `switch_generation: u64`。連打時は最後の switch だけが適用される
   (古い background 結果は generation 不一致で破棄)。watcher の generation 機構とは独立
5. **キャッシュの鮮度**: watcher は active repo のみ監視のため、非 active tab は staleness を
   許容する(切替時の revalidate で回復)。reload() はキャッシュも更新する
6. (stretch / later)watcher 起因の `reload_external` も同じ background 経路に乗せ、
   大 repo での自動 refresh のジャンクも解消する

## Consequences

- per-repo 状態の「純データ部分」を `TabViewState` として括り出すリファクタが必要
  (KagiApp のフィールド群は維持し、apply 関数で一括代入する形 — 構造の大手術はしない)
- メモリ: tab 数 × rows/details。10k commits × 数 tab は実用上問題ない規模
- 適用(apply)自体は main スレッドだが代入のみで軽い
