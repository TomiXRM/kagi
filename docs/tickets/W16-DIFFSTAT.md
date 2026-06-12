# W16-DIFFSTAT: Per-file Diffstat + Diffstat Mini Bar

- Status: in-progress
- 担当: worktree agent(Opus)
- チケット: T-DIFFSTAT-001〜007(requirements-diffstat.md が仕様の正)

## スコープ境界

- 表示場所は**優先 1・2 のみ**(Inspector Changed Files / Commit Panel staged・unstaged)。
  Commit Preview / Compare View への統合は後段で PM が行う(W14-PREVIEW と並行のため)
- 集計は git2 `Patch::from_diff` + `line_stats()`。inspector は MAX_FILES 切り詰め後のみ計算
- 触ってよいファイル: `src/git/diffstat.rs`(新規)/ `src/git/mod.rs`(re-export)/
  `src/ui/diffstat_bar.rs`(新規)/ `src/ui/inspector.rs` / `src/ui/commit_panel.rs` /
  `src/ui/file_tree.rs`(行データに stat を通す場合)/ `tests/diffstat_test.rs`(新規)/ 担当チケット
- Tooltip は gpui-component Tooltip(`.id` 必須)

## 共通規約(全 lane 同一)

- 破壊的 git 操作の実装禁止(`--force` / `reset --hard` / `git clean`)。確認なし実行禁止
- 検証は `scripts/make_fixture.sh` の fixture / tempdir のみ。**ユーザー repo 禁止**
- 文字列切り詰めは `chars()` ベース。色は theme() 経由(ハードコード禁止)
- `cargo test` は exit code を確認(パイプで握りつぶさない)。own-code warning 0
- macOS に `timeout` コマンドはない。`cargo build` 後の GUI 起動確認は PM が行う
- 完了時: 担当チケット末尾に実装メモ追記 + Status 更新、worktree branch に commit
