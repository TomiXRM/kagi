# T-DISCARD-004: Discard headless 経路(KAGI_DISCARD / KAGI_DISCARD_ALL)

- Status: todo
- 依存: T-DISCARD-001
- 関連: ADR-0046

## スコープ

- `src/main.rs` に `KAGI_DISCARD=<path>` / `KAGI_DISCARD_ALL=1`(+ KAGI_AUTO_CONFIRM)経路を追加。
  既存 KAGI_* と同形式の `[kagi] planned/executed/verified:` ログ

## 完了条件

- [ ] fixture で headless 実行ログが取れる(PM の回帰網に載る)
- [ ] `cargo test` 全パス、own-code warning 0
