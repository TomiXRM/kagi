# W18-COAUTHOR-COPY: Co-author 表示 + commit hash チップの click-to-copy

- Status: in-progress
- 担当: worktree agent(Opus)
- 発端: ユーザー指示 2026-06-13(原文準拠)
  1. 「co-author の表示をできるようにしておいて」
  2. 「commit detail pane に copy SHA があるが、commit hash が表示されているので、それをクリックしたら
     コピーされるようにしておいて。copy したらスナックバーで copyed 通知するように」

## スコープ

### 1. Co-author 表示(inspector / commit detail)

- commit message の trailer `Co-authored-by: Name <email>`(大文字小文字非依存、複数行可)を
  純関数で parse(`src/git/` 側、UI 非依存。unit test 付き)
- inspector の author メタ行の下/隣に co-author を表示(名前 + email tooltip、
  GitHub avatar 機構(avatar.rs / avatar_images)が email から引けるなら同じ 16px avatar を出す。
  引けない場合はイニシャル円で可)
- 0 件なら何も出さない。表示は author 行より控えめ(text_xs / muted)

### 2. Hash チップ click-to-copy + toast

- inspector の hash チップ(短 SHA 表示)クリックで **full SHA をクリップボードへ**
  (既存 `context_menu::copy_full_sha` 相当を再利用)
- コピー成功で **toast(スナックバー)「Copied <short sha>」**(`push_toast(ToastKind::Info, ...)`)
- 既存の「Copy SHA」アクションボタンも同じ toast を出すよう統一(挙動は不変、通知だけ追加)
- hover で cursor_pointer + 軽い強調(クリックできることが分かるように)

## 触ってよいファイル

- `src/ui/inspector.rs` / `src/ui/mod.rs`(toast 配線)/ `src/ui/context_menu.rs`(copy 関数再利用のみ)
- co-author parse: `src/git/log.rs` か新規 `src/git/trailers.rs` + `src/git/mod.rs` re-export
- `tests/trailers_test.rs`(新規、parse の unit test)
- `docs/tickets/W18-COAUTHOR-COPY.md`

## 共通規約

- 破壊的 git 操作の実装禁止。fixture / tempdir のみで検証(ユーザー repo 禁止)
- 文字列切り詰めは chars() ベース。色は theme() 経由。own-code warning 0
- `cargo test` は exit code 確認。macOS に timeout コマンドなし
- 完了時: 本チケット末尾に実装メモ + Status: done、worktree branch に commit(push/merge しない)
