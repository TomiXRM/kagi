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

## 実装メモ (done)

- Status: done
- Inspector(`src/ui/inspector.rs`)Path/Tree 両モードのファイル行末に `diffstat_unit` を追加。path 側は `flex_1().min_w(0)` + truncate で bar と共存。
- diffstat は `KagiApp.diffstat_cache: HashMap<usize, Vec<FileDiffStat>>`(`commit_diffstat` で diff_cache と同時に lazy 計算)→ render で truncated set のみ参照(MAX_FILES 切り詰め後計算の要件充足)。
- Commit Panel(`src/ui/mod.rs::render_commit_panel`)staged/unstaged の tree/flat 4 経路すべてに追加。stats は `CommitPanelState.{staged,unstaged}_stats`(reload_status で `staged_diffstat`/`unstaged_diffstat`)。path 引きは `find_stat`。
- 集計は既存 fetch 経路に同居。Compare View は対象外(`compare_for_panel.is_some()` で diffstat=None)。
