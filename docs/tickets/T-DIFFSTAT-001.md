# T-DIFFSTAT-001: FileDiffStat model を定義する

- Status: todo
- 関連: requirements-diffstat.md、lane W16-DIFFSTAT

## スコープ

- `src/git/diffstat.rs`(新規)に `FileDiffStat { path, change: ChangeKind, additions, deletions, is_binary }` を定義
- UI 非依存の純データ。`src/git/mod.rs` から re-export

## 完了条件

- [ ] model 定義 + re-export、`cargo test` 全パス、own-code warning 0
