# T-COMMIT-001: Commit Preview — staged 概要(count / summary / target branch / author)

- Status: todo
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
