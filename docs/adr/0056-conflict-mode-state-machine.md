# ADR-0056: Conflict Mode State Machine(一級状態)

- Status: Accepted(2026-06-13、実装はユーザー go 後)
- 根拠: requirements-conflict-ux.md §3 / research/conflict-ux-models.md

## Decision

- `RepoMode { Normal, Conflict(ConflictSession) }` を KagiApp(tab ごと)に追加。
  ConflictSession = { op(Merge/Rebase{step,total}/CherryPick/Revert)、files(kind: content /
  rename-delete / modify-delete / binary、status: unresolved/resolved/needs-review)、resolution buffer }
- 検出: `Repository::state()` + `Index::conflicts()`。**CLI 等の外部起因も** 起動時・watcher で検出
- Mode 中はアプリ全体が反応: header 常設バナー(op 名 + 進捗 N/M + Continue/Abort/Skip)、
  sidebar conflict 表示、graph 対象 commit 強調、危険操作 disabled(BCM の conflict_mode と接続)
- continue = バッファ書き出し → marker 検査(checklist 再利用)→ stage → 各操作の継続。
  **全ファイル解決まで continue 無効(KDiff3 流)**。abort = `cleanup_state` + 開始前 snapshot へ
  (常時可能・plan 経由・oplog 記録)。skip は sequencer 系のみ
