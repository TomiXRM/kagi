# T-DISCARD-004: Discard headless 経路(KAGI_DISCARD / KAGI_DISCARD_ALL)

- Status: done
- 依存: T-DISCARD-001
- 関連: ADR-0046

## スコープ

- `src/main.rs` に `KAGI_DISCARD=<path>` / `KAGI_DISCARD_ALL=1`(+ KAGI_AUTO_CONFIRM)経路を追加。
  既存 KAGI_* と同形式の `[kagi] planned/executed/verified:` ログ

## 完了条件

- [x] fixture で headless 実行ログが取れる(PM の回帰網に載る)
- [x] `cargo test` 全パス、own-code warning 0

## 実装メモ

- `src/main.rs` に `run_headless_discard(repo_path, single, all)` を追加し、KAGI_POP 経路の直後に
  `KAGI_DISCARD=<path>` / `KAGI_DISCARD_ALL=1` を判定して呼ぶ。注意: 既存 KAGI_* 同様、
  repo は **CLI 第1引数**で渡す(`KAGI_OPEN_REPO` ではなく `kagi <repo>`)。
- ログは既存形式: `[kagi] planned: discard N target(s), blockers=.. warnings=.. destructive=true`
  → (AUTO_CONFIRM 時) `[kagi] executed: discarded N file(s); backup: path=<sha>, ...`
  → `[kagi] verified: N target(s) left the unstaged set` → `[kagi] footer: discard: .. (Success)`。
  blocker あり時は `[kagi] KAGI_AUTO_CONFIRM=1 but discard has N blocker(s), skipping` + Refused。
- KAGI_DISCARD_ALL は status から untracked/conflicted を除いた unstaged 全件を対象にし、
  KAGI_DISCARD の単一 path も(重複しなければ)併合。oplog は record_headless_op で op="discard"。
- fixture 検証ログ(抜粋):
  ```
  [kagi] planned: discard 1 target(s), blockers=0 warnings=1 destructive=true
  [kagi] executed: discarded 1 file(s); backup: b.txt=9a27028f37db56bdf66cc2fe5a917055a62a1acd
  [kagi] verified: 1 target(s) left the unstaged set
  [kagi] footer: discard: branch: main → branch: main (Success)
  ```
  untracked.txt 指定時は `blockers=1 ... (Refused)`、untracked ファイルは WT に残存(git clean 不実装規約維持)。
