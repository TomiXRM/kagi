# W17-DISCARD: Discard unstaged changes(backup-then-discard)

- Status: in-progress
- 担当: worktree agent(Opus)
- チケット: T-DISCARD-001〜004(ADR-0046 が意味論の正。**必読**)

## 絶対条件

- **backup-then-discard**: ODB blob backup なしで WT を上書きする経路を作らない。backup 失敗 = 全体中止
- untracked は削除しない(git clean 禁止規約)。conflicted は blocker
- staged(index)と refs には一切触れない
- async 実行は W15 パターン(`start_*` + blocking core free fn、busy_op、headless は sync)を踏襲。
  実例: mod.rs の `stash_pop_blocking` / `start_pop`
- 触ってよいファイル: `src/git/ops.rs` / `src/git/oplog.rs`(op 種別追加が要るなら)/
  `src/ui/mod.rs` / `src/ui/commit_panel.rs` / `src/main.rs`(headless)/
  `tests/discard_test.rs`(新規)/ 担当チケット

## 共通規約(全 lane 同一)

- 破壊的 git 操作の実装禁止(`--force` / `reset --hard` / `git clean`)。確認なし実行禁止
- 検証は `scripts/make_fixture.sh` の fixture / tempdir のみ。**ユーザー repo 禁止**
- 文字列切り詰めは `chars()` ベース。色は theme() 経由(ハードコード禁止)
- `cargo test` は exit code を確認(パイプで握りつぶさない)。own-code warning 0
- macOS に `timeout` コマンドはない。`cargo build` 後の GUI 起動確認は PM が行う
- 完了時: 担当チケット末尾に実装メモ追記 + Status 更新、worktree branch に commit
