# W14-PREVIEW: Commit Preview(staged 概要 + staged diff preview)

- Status: done
- 担当: worktree agent(Opus)
- チケット: T-COMMIT-001 / T-COMMIT-002(各ファイル参照。完了条件・触ってよいファイルはチケットが正)
- 関連: ADR-0039、requirements-commit-suite.md

## 補足(チケットからの差分)

- Commit Panel は wave1 で checklist / draft autosave / smart commit が入っている。既存挙動を壊さない
- staged diff preview は既存 diff viewer(main-pane の highlight 付き)を**再利用**。新規レンダラ禁止
- author 欠損 repo は "(unknown)" fallback、panic 不可

## 共通規約(全 lane 同一)

- 破壊的 git 操作の実装禁止(`--force` / `reset --hard` / `git clean`)。確認なし実行禁止
- 検証は `scripts/make_fixture.sh` の fixture / tempdir のみ。**ユーザー repo 禁止**
- 文字列切り詰めは `chars()` ベース。色は theme() 経由(ハードコード禁止)
- `cargo test` は exit code を確認(パイプで握りつぶさない)。own-code warning 0
- macOS に `timeout` コマンドはない。`cargo build` 後の GUI 起動確認は PM が行う
- 完了時: 担当チケット末尾に実装メモ追記 + Status 更新、worktree branch に commit
