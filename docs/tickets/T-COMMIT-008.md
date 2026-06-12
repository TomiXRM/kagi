# T-COMMIT-008: Draft Autosave — UI 配線(debounce 保存 / 復元 / branch 切替 / 成功時 clear)

- Status: todo(UI 配線は主に PM が main 側。本チケットは backend 連携点の規定 + 配線案)
- 依存: T-COMMIT-007 / ADR-0042 / 既存 `schedule_modal_replan`(debounce 機構)
- 関連: lane W14-DRAFT

## 背景

drafts backend を Commit Panel に繋ぐ。Input 変更 → 250ms debounce → background 保存。repo open / branch 切替で
復元。commit/amend 成功で clear。

## スコープ(配線規定)

- message Input 変更時に **250ms debounce**(generation counter + `gpui::Timer` + 最新世代のみ、
  `schedule_modal_replan` を参考)した後 `save_draft` を `cx.background_spawn` で呼ぶ。
- repo open / branch 切替時に `load_draft` → Input が空なら流し込む(非空ならユーザー入力優先で上書きしない)。
- branch 切替時: 旧 branch を `save_draft` → 新 branch を `load_draft`。
- `execute_commit` / `execute_amend` 成功時に該当 branch を `clear_draft`(失敗時は残す)。

## 完了条件

- [ ] 入力 → 250ms 後に draft が保存される(連打で 1 回に集約)
- [ ] 再起動相当(repo 再 open)で復元される
- [ ] branch 切替で draft が分離される
- [ ] commit 成功で draft が消える
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs` / `src/ui/mod.rs`(debounce スケジューラ / 配線)
- `docs/tickets/T-COMMIT-008.md`

## 触ってはいけないファイル

- `src/git/drafts.rs`(backend は T-COMMIT-007 で確定済み、API のみ使う)/ `Cargo.toml`

## テスト方法

1. `cargo test`(配線は UI のため、PM がスクリーンショット + headless ログで確認)
2. tempdir / `KAGI_LOG_DIR` で draft 先を隔離

## リスク・規約

- 復元時にユーザーの入力を踏み潰さない(空判定を厳密に)
- background 保存で UI を塞がない(avatar / oplog と同様)
