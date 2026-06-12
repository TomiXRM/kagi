# ADR-0010: Ahead/Behind State Calculation

- Status: Accepted
- Date: 2026-06-12

## Context

Header Toolbar と Status Bar に ahead/behind を常時表示する(requirements-shell.md §4)。
ahead/behind は **ローカルにある remote-tracking ref との比較**であり、fetch しない限り
リモートの実態と乖離する。この限界をどう扱うかを決める。

## Decision

1. **計算**: 既存 T005 の `graph_ahead_behind(local_tip, upstream_tip)` を唯一の計算経路とする。
   表示は `branch ↑A ↓B` / `no upstream` / `detached HEAD` の3形態
2. **鮮度の明示**: Status Bar に**最終 refresh 時刻**(T029 の watcher による更新も含む)を表示する。
   behind 値の隣に「local データ基準」であることを示す(ツールチップ or 表記)。
   **自動 fetch はしない**(ネットワークを勝手に触らない方針。fetch は明示操作のみ)
3. **更新タイミング**: snapshot 再取得(操作後 reload / 外部変更 watcher / 手動 Refresh)の都度。
   ahead/behind 専用の差分計算パスは作らない(snapshot に含まれているため)
4. **detached HEAD**: ahead/behind 非表示 + 「detached HEAD」表示。Pull / Push / Undo Commit を disabled
5. **unborn**: 「no commits yet」を表示し、Pull / Push を disabled

## Consequences

- 「behind 0 なのに実は遅れている」は fetch するまで起こりうる — 鮮度表示で許容(GitKraken も同様)
- fetch を実装する際(T-HT-003 Pull の一部 or 独立)、fetch 後に watcher が refs/remotes の変化を拾い
  自動で表示が更新される(T029 の副産物)
