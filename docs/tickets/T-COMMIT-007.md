# T-COMMIT-007: Draft Autosave — backend(branch ごと保存・復元・clear)

- Status: todo
- 依存: ADR-0042 / 既存 oplog path 解決
- 関連: lane W14-DRAFT

## 背景

書きかけ commit message を branch ごとに保存し再起動後も復元、commit 成功時に clear する。
保存先・形式は oplog 流儀(手書き JSON・serde 禁止・`~/.kagi/`・env override)。

## スコープ

- 新 module `src/git/drafts.rs`:
  ```rust
  pub struct Draft { pub repo: String, pub branch: String, pub message: String, pub mode: String, pub updated: u64 }
  pub fn save_draft(repo_path: &Path, branch: &str, message: &str, mode: &str) -> Result<(), GitError>;
  pub fn load_draft(repo_path: &Path, branch: &str) -> Option<Draft>;
  pub fn clear_draft(repo_path: &Path, branch: &str) -> Result<(), GitError>;
  ```
- 保存先: `$KAGI_LOG_DIR/drafts/` → なければ `$HOME/.kagi/drafts/`(oplog path 解決を踏襲)。
- ファイル名: `sha1(repo_path + "\0" + branch).json`。形式: 手書き JSON(serde 禁止、oplog の writer/parser 流用)。
- 空 message(trim 後空)で `save_draft` → ファイル削除。壊れた JSON は無視(空扱い、commit を妨げない)。

## 完了条件

- [ ] save → load round-trip(message / mode / branch 復元)
- [ ] 別 branch / 別 repo で衝突しない(ファイル名キー)
- [ ] clear でファイル削除。空 message save で削除
- [ ] 壊れた JSON を load → None(panic しない)
- [ ] unit test: round-trip / branch 分離 / clear / 空削除 / 壊れ JSON、計 5+(`KAGI_LOG_DIR` で隔離)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/drafts.rs`(新規)/ `src/git/mod.rs`(re-export)/ `src/git/oplog.rs`(path 解決・JSON ヘルパの共有のみ、既存挙動不変)
- `tests/drafts_test.rs`(新規)
- `docs/tickets/T-COMMIT-007.md`

## 触ってはいけないファイル

- `src/ui/*` / `src/main.rs` / 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(`KAGI_LOG_DIR` を tempdir に。oplog テストの直列化パターンを参考)
2. tempdir のみ。ユーザーの `~/.kagi/` を汚さない

## リスク・規約

- serde 禁止(手書き JSON)。エスケープは oplog writer を再利用(`"` / `\` / 制御文字 / 改行)
- `KAGI_LOG_DIR` を使うテストは oplog テストと同じく直列化(env 競合回避)
