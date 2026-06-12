# T-DIFFSTAT-007: binary / renamed / deleted / conflicted file の fallback 表示を実装する

- Status: todo
- 依存: T-DIFFSTAT-005

## スコープ

- binary → `BIN`(bar なし)
- renamed → `R` + rename 元/先(tooltip でフルパス)
- conflicted → `U` または warning icon を diffstat より優先
- deleted → 赤のみ bar + `-N`

## 完了条件

- [ ] fixture で 4 ケースを再現し表示が破綻しない、`cargo test` 全パス

## 実装メモ (done)

- Status: done
- binary → `BIN`(bar なし)。renamed → 行頭バッジ `R`(既存 change_badge / status_badge)+ bar は rename の内容変更分を表示、tooltip でフル数値。
- conflicted → Commit Panel は `is_conflicted` 優先で `C` バッジ+赤 tint 行(diffstat より優先、既存ロジック維持)。
- deleted → `bar_segments(0, N, ..)` で赤のみ + `-N`。
- fixture(diffstat_test.rs)で add/modify/delete/binary/rename を再現、表示破綻なし(集計値検証)。`cargo test` 全 green。
