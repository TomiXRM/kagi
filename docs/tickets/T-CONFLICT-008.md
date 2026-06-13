# T-CONFLICT-008: continue / abort の plan 統合

- Status: backend-done(W26-CONFLICT-CORE の backend 半分。banner/panel UI wiring は後続レーン)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

continue = バッファ書き出し→stage→操作継続(merge commit / sequencer)。abort = cleanup_state + 開始前復帰。両方 plan→oplog 経由 + バッファ退避

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証

## 実装メモ(backend-done)

- `src/git/conflicts.rs` に `OperationPlan` パイプライン準拠の plan_/execute_ ペアを実装(NO UI wiring)。
- `plan_conflict_continue`: blocker = (1) 未解決ファイル(buffer に Result 無し)(2) marker 残存
  (`files_with_marker_residue`)。KDiff3 流「全解決まで continue 無効」。
- `execute_conflict_continue`: buffer の resolved テキストを WT へ書出し→`index.add_path` で stage
  (stage1/2/3→0)。merge は HEAD+MERGE_HEAD の merge commit を作成 + `cleanup_state` →
  `ContinueOutcome::Committed`。sequencer(rebase/cherry-pick/revert)は stage 済みで `Staged` を返す
  (commit + 次 pick 前進は後続の sequence executor レーン)。marker は execute 時にも防御再チェック。
- `plan_conflict_abort`(blocker 無し=常時可)/ `execute_conflict_abort`: ① buffer を autosave dir へ
  退避(`AbortOutcome.buffer_preserved_at` を oplog 用に返す)② ORIG_HEAD の tree を index に read_tree
  して conflict stage を解消 ③ 衝突 path のみ pre-op blob を WT へ書戻し(force/reset --hard/clean 不使用、
  対象 path 限定の安全復元)④ branch ref を ORIG_HEAD へ戻し `cleanup_state`。
- 検証: `tests/conflicts_test.rs`(continue gate の未解決/marker/clean、merge continue が 2-parent commit、
  abort が ORIG_HEAD へ復元しマーカー残らず・MERGE_HEAD 消去・buffer 退避ファイル存在)。
