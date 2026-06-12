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

## 実装メモ(2026-06-13)

- 新規 `src/git/drafts.rs`(backend のみ)。`src/git/mod.rs` に `pub mod drafts;` + `pub use drafts::{Draft, clear_draft, load_draft, save_draft};` を追加。`src/ui/*` / `src/main.rs` / `staging.rs` は不変(UI 配線は別 lane)。
- 保存先: `$KAGI_LOG_DIR/drafts/` → なければ `$HOME/.kagi/drafts/`(oplog の path 解決を踏襲)。
- ファイル名キー: `sha1(repo_path + "\0" + branch).json`。`Cargo.toml` 凍結のため sha1 crate は使わず、自前 SHA-1(RFC 3174、ファイル名用途のみ。known-answer test 3 本で検証: 空 / "abc" / 2-block)。
- 形式: 手書き JSON `{"repo","branch","message","mode","updated"}`。エスケープは oplog と同方式(`"` `\` `\n` `\r` `\t` `\uXXXX`)。読みは寛容パーサ(壊れたら `None` = draft 無視、commit を妨げない)。`branch`/`message` のみ必須、`repo`/`mode`/`updated` は省略時デフォルト。
- 挙動: 空(trim 後空)message の `save_draft` はファイル削除に委譲。`clear_draft` は不在でも no-op で成功。
- テスト: lib unit(純関数 6 本: sha1 KAT×3 / JSON round-trip / 非 object 拒否 / lenient default、+ path-key 検証 1)+ integration `tests/drafts_test.rs`(7 本: round-trip / branch×repo 分離 / clear / 空削除 / 壊れ JSON / 不在 None / 不在 clear no-op)。
- env 競合回避: `KAGI_LOG_DIR` を触る file-backed テストは lib 内に置かず integration binary に集約(別プロセスで oplog の env テストと直列化不要に)。`with_log_dir` で tempdir 隔離 + restore + ENV_LOCK。
- 検証: `cargo test` 全パス(2 連続 green)、own-code warning 0。実物 draft ファイルを tempdir に materialize して schema / 40-hex 名 / save→load→clear を確認済み。
