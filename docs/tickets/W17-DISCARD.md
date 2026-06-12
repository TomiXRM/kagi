# W17-DISCARD: Discard unstaged changes(backup-then-discard)

- Status: done
- 担当: worktree agent(Opus)
- チケット: T-DISCARD-001〜004(ADR-0046 が意味論の正。**必読**)— 全 done

## 完了サマリ

- backend(ops.rs): `plan_discard` / `execute_discard`(`DiscardBackup`/`DiscardOutcome`)。
  実行順は ADR 厳守 backup(`repo.blob`、失敗=全体中止)→ `checkout_index`(path+force,
  `update_index(false)` で index 不変・refs 不変)→ verify。oplog op="discard" に path→blob SHA を
  after.dirty で記録(復元ハンドル、`git cat-file -p` で取り出せる)。
- UI(mod.rs / commit_panel): per-file 赤 Discard ボタン(unstaged 行のみ、untracked/conflicted 除外)、
  danger 確認 modal(赤・ESC cancel・**backdrop と card 両方 occlude**・対象一覧スクロール・skipped 明示・
  0 件で Discard 非表示)、Discard all ヘッダボタン(0 件 disabled)。実行は W15 async パターン
  (`start_discard` + `discard_blocking` free fn、busy_op="discard"、toast、reload)。複数=1 オペ=1 oplog。
- headless(main.rs): `KAGI_DISCARD=<path>` / `KAGI_DISCARD_ALL=1`(+ KAGI_AUTO_CONFIRM)。
  `[kagi] planned/executed/verified/footer:` ログ。
- tests/discard_test.rs(新規 7 tests)+ 既存全 suite green(計 24 suite)。own-code warning 0。
- 触ったファイル: src/git/ops.rs, src/git/mod.rs(re-export), src/ui/mod.rs, src/main.rs,
  tests/discard_test.rs, docs/tickets/T-DISCARD-001..004 + W17-DISCARD.md。oplog.rs はスキーマ変更不要のため未改変。

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
