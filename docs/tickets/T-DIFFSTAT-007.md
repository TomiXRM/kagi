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
