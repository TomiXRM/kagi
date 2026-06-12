# T-DIFFSTAT-005: Changed Files list に status / additions / deletions / bar を表示する

- Status: todo
- 依存: T-DIFFSTAT-002/004
- 関連: requirements-diffstat.md「表示場所」優先 1・2

## スコープ

- Inspector の Changed Files list(`src/ui/inspector.rs`、Path/Tree 両モード)の各ファイル行右端に
  `+N -M [bar]` を追加(file path より目立たせない)
- Commit Panel の staged/unstaged list にも同様に追加
- 集計は既存 fetch 経路に同居(inspector は MAX_FILES 切り詰め後のみ計算)

## 完了条件

- [ ] Inspector / Commit Panel 両方で表示、横幅が細くても path の truncate と共存
- [ ] `cargo test` 全パス、own-code warning 0
