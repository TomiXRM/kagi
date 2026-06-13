# ADR-0068: Conflict Save / Continue / Commit / Abort の責務分離

- Status: Accepted(2026-06-13、ユーザー指摘: 現状 Continue が即 merge 完了で GitKraken と違う)
- 関連: requirements-conflict-ux.md v2 / ADR-0056/0057/0067 / commit panel(T025〜)

## 問題

現状 `続行(Continue)` が即 merge commit を作る。GitKraken は「各ファイルを Save で解決→stage、
全部 resolved 後に commit message 画面→merge commit 作成」という多段。Save/Continue/Commit が混在。

## Decision: 4 操作を別物として定義

| 操作 | 意味 | 結果 |
|------|------|------|
| **Save resolution**(file) | 選択ファイルの Result を **working tree へ書く** + marker 検査 + **index の unmerged entry を解消するよう stage** | そのファイルが Resolved Files へ移動。merge commit は作らない |
| **Continue**(operation) | 全 conflict が resolved/staged になった後に進む | **merge: commit message 画面へ遷移**(即 commit しない)。rebase/cherry-pick: `--continue` の OperationPlan(確認画面)を出す |
| **Commit**(merge のみ) | commit message を確定 | **merge commit を作成**(既存 commit panel / commit フローに乗せる) |
| **Abort** | 操作中止 | merge/rebase/cherry-pick を中止し pre-op 状態へ(ADR-0056/0067、cleanup_state + ORIG_HEAD) |

### Save resolution の詳細(file 操作)
- Result Preview の内容を WT へ書き、`<<<<<<< ======= >>>>>>>` 残存を検査(残存は blocker)
- git index の当該 path の unmerged(stage 1/2/3)を解消し **stage 0 に add**(`index.add_path` 相当)
- 対象ファイルを session の Resolved Files に移す。Continue 可能条件を再評価。Operation Log に記録
- まだ merge commit は作らない

### Continue の詳細(operation 操作)
- 全ファイル resolved & staged & marker 無し & index に unmerged 無しが条件(ADR-0067)
- **merge**: いきなり commit せず、**commit message panel(merge message プリフィル)へ遷移**。
  ユーザーが message 編集 → `Commit merge` で merge commit 作成(2 親: HEAD + MERGE_HEAD)
- **rebase / cherry-pick / revert**: `git <op> --continue` 相当の OperationPlan を生成して確認画面 →
  実行(sequencer 継続)

## Consequences
- W26 の `plan_conflict_continue`(merge を即 commit)を分割: continue(=遷移/plan)と commit(=作成)。
- merge commit 作成は commit panel 側に寄せ、checklist(ADR-0043)も通せる。
- Save が index stage まで行うので、外部 CLI とも整合(`git status` が resolved を反映)。
