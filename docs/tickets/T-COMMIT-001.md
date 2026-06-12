# T-COMMIT-001: Commit Preview — staged 概要(count / summary / target branch / author)

- Status: done
- 依存: 既存 Commit Panel(T025〜T027)/ staging API
- 関連: ADR-0039、requirements-commit-suite.md、lane W14-PREVIEW

## 背景

「間違った変更をコミットしない」の柱。commit 前に **何が staged されているか**を Commit Panel に常設表示し、
plan modal にも要約を出す。staged diff preview は T-COMMIT-002 に分離。

## スコープ

- Commit Panel に **staged files count** / **changed files summary**(A=追加 / M=変更 / D=削除 を件数で)/
  **target branch**(現在 HEAD branch、detached なら短 SHA + 「detached」)/ **author**(repo config の
  user.name / user.email)を表示。
- これらは既存 staging snapshot から純粋に組み立て(新 git 操作なし)。author は `repo.signature()` 相当から取得。
- plan modal(`plan_commit` の予測表示)にも count / summary を出す(既存フィールドで表現)。

## 完了条件

- [ ] count / A/M/D summary / target branch / author が Commit Panel に表示される
- [ ] detached HEAD / unborn でも壊れずに表示(branch 欄に適切な文言)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 既存 headless 検証に回帰なし
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs`(UI)
- 必要なら `src/git/staging.rs`(author/summary を返す純関数の追加。既存 plan を壊さない)
- `docs/tickets/T-COMMIT-001.md`

## 触ってはいけないファイル

- 上記以外(特に `Cargo.toml` / 他チケットのファイル / `scripts/*`)

## テスト方法

1. `cargo test`(exit code 確認、パイプで握りつぶさない)
2. fixture(`scripts/make_fixture.sh`)/ tempdir のみ。ユーザー repo 禁止
3. UI は PM がスクリーンショット確認(headless ログ併設)

## リスク・規約

- author は config 未設定の repo があり得る → 欠損時は「(unknown)」等で fallback、panic 不可
- 文字列切り詰めは `chars()` ベース(byte slice は日本語で panic)

## 実装メモ(done)

- 純関数 `commit_preview(repo) -> Result<CommitPreview, GitError>` を `src/git/staging.rs` に追加。
  既存 `working_tree_status` + `resolve_head` + `repo.config()` の読み取りのみで組み立て(新規 git 操作なし)。
  - `CommitPreview { staged_count, added, modified, deleted, other, target_branch, author }`。
  - staged の `ChangeKind` を A/M/D に振り分け。Renamed/TypeChange は `other` に集約。
  - `target_branch`: attached → branch 名 / unborn → `"<branch> (unborn)"` /
    detached → `"<short8 sha> (detached)"`。短縮は `chars().take(8)` で byte slice panic 回避。
  - `author`: `user.name` / `user.email` から `"Name <email>"`。両方欠損 or config open 失敗 → `"(unknown)"`(panic 不可)。
  - `CommitPreview::summary()` で `"+a ~m -d"` 文字列(staged 0 件 → 空文字)。
  - `src/git/mod.rs` で `commit_preview` / `CommitPreview` を re-export。
- UI(`src/ui/mod.rs` `render_commit_panel`):commit footer 先頭に preview ブロックを追加。
  - 行1: `N file(s) staged` + A/M/D summary、行2: `→ <target_branch>`、行3: `by <author>`。色は全て theme() 経由。
  - preview は呼び出し側(KagiApp::render)で `commit_preview` を実行して `Option<CommitPreview>` を渡す。
    repo open 失敗時は None → ブロック非表示(空 div)。
- plan modal: 既存 `OperationPlan.preview_files`(= staged FileStatus)で count / A/M/D を既に表現済みのため
  新規フィールド追加なし(チケットの「既存フィールドで表現」に合致)。
- テスト(`tests/staging_test.rs`, 4 件追加):
  - `test_commit_preview_amd_counts_attached`(A/M/D 各1 + branch=main + author)
  - `test_commit_preview_unborn_head`(`main (unborn)`)
  - `test_commit_preview_detached_head`(`<8hex> (detached)`)
  - `test_commit_preview_author_unknown_fallback`(global config を `git2::opts::set_search_path` で空 dir に
    退避し identity 無し repo を作って `(unknown)` を検証。process-global なので static Mutex で直列化、終了後 reset)。
- 検証: `cargo build` own-code warning 0 / `cargo test` 全 suite green(exit 0)。fixture/tempdir のみ使用。
