# ADR-0215: sync-to-remote slice 1 — git2 だけで「保全してから揃える」

- Status: **Accepted**（slice 1 = backend。UI は slice 2、CLI / MCP 露出なし）
- Date: 2026-10-02
- Related: [#536](https://github.com/TomiXRM/kagi/issues/536)、[ADR-0203](0203-sync-to-remote.md)（設計 draft。本 ADR はその保全方針を実装可能な最小形に絞る）、ADR-0104（pipeline）、ADR-0023（destructive = 二段 confirm）、#523 / ADR-0179（`refs/kagi/backups/`）、ADR-0154（snapshot は必須保全に使わない）
- 不変条件 3: `reset --hard` / `git clean` / `push --force` の文字列はコードにも文書にも出さない（CI gate）。本操作はそれらの代替であり、どれも使わない。

## 0. アーキテクチャレビュー

1. **SEARCH.** 既存の「ref だけ動かす」= `ops/reset.rs::execute_reset_current_to_head`（CAS なし、index / WT に触らない）。「WT を置き換える」= `ops/snapshot.rs::execute_restore_snapshot`（`checkout_tree` + `force`、HEAD は動かさない）。「消去前に pin」= `ops/branch.rs::execute_delete_branch`（`retain_object` → lock → expected-tip 比較 → write）。stash 形式の commit を git2 で書く箇所は無い（`ops/stash_push.rs` は CLI）。
2. **OWNER.** 複合 git write の境界 = `crates/kagi-git/src/ops/<feature>.rs` の triple + `Backend::run`。保全 = `ops/backup.rs`。
3. **DECISION.** EXTEND: `ops/sync_to_remote.rs` に `plan_ / preflight_ / execute_ / verify_sync_to_remote`、`Operation::SyncToRemote { branch }`、`OperationOutcome::SyncToRemote`、plan-note family `Sync`。新 crate / manager なし。
4. **REFACTOR.** `ops/snapshot.rs::write_worktree_tree` を `pub(crate)` にして再利用（`git add -A` 相当の tree を、ディスク上の index を書かずに作る）。
5. **ANTI-PATTERNS.** #5: 「HEAD の branch かどうか」は分岐ではなく「WT 保全と置換を行うか」のパラメータ。#7: snapshot と別の保全経路を作ったが、目的が違う（snapshot = 任意・cap 付き・stage 分離なし / 本 ADR = 必須・forget まで残る・stage 分離あり）。ADR-0203 の archive manifest 形式は採らない（§3）。

## 1. 利用者への約束（slice 1）

- 対象: ローカル branch `B`（HEAD でもそうでなくてもよい）と、その upstream `R`（取得済み remote-tracking ref）の tip `T`。
- 結果: `B = T`。`B` が HEAD なら index と tracked WT も `T` に一致し、staged / unstaged は空。
- 保全（必須、設定に依らない）: `refs/kagi/backups/<op>/0` = 旧 tip `H`。`B` が HEAD で dirty なら `refs/kagi/backups/<op>/1` = **git stash と同じ形の commit**（tree = WT 全体、親 1 = `H`、親 2 = index commit）。復元は標準 git だけで行える:
  - `git update-ref refs/heads/<B> refs/kagi/backups/<op>/0`
  - `git stash apply --index refs/kagi/backups/<op>/1`
  テストはこの 2 コマンドだけで status（`A`/`MM`/`D `+`??`/`??`）、index entries、WT 内容が完全に元に戻ることを確認する。
- **untracked（非 ignore）は backup/1 に取り込んだ上で WT から取り除く。ignored には一切触らない。** 取り除くのは「現在のファイル内容の hash が backup/1 の tree の blob と一致する」と検証できたパスだけで、1 ファイルずつ `remove_file` する。一括掃除はしない。ignored ファイルが target のパスと衝突する場合は checkout が拒否し（`overwrite_ignored(false)`）、ref は動かない。
- ネットワークは使わない。`T` は plan 時点で取得済みの値。plan 後に `R` が動けば preflight が拒否する。

## 2. 実行順序（ADR-0203 の順序を採用、PM 案から 1 点変更）

| 段 | 処理 | 失敗時 |
|---|---|---|
| 1 | preflight: blockers 空、`head_at_plan` 一致、`B` が plan 時の `H`、`R` が plan 時の `T`（title の short OID）、進行中操作なし、conflict なし、plan が clean 前提なら今も clean | Refused（write なし） |
| 2 | `retain_object(op, 0, H)`。`B` が HEAD で dirty なら index commit + WT commit を書き `retain_object(op, 1, S)` | backup ref は oplog に残る |
| 3 | `B` が HEAD: `checkout_tree(T, force, overwrite_ignored=false, remove_untracked=false)` | Partial（`partial_after` に復元コマンド）。**ref はまだ動いていない** |
| 4 | backup/1 の tree と一致する untracked を個別に削除 | 同上 |
| 5 | `refs/heads/B` を lock → 今も `H` か再確認 → `set_target(T)` → commit | 動いていれば Partial（WT は T、branch は H） |
| 6 | verify: `B == T`、HEAD の場合 staged/unstaged/conflicted 空 + 保全した untracked が残っていない、backup ref が両方正しい OID | Partial |

PM 案は「(3) ref 更新 → (4) checkout」だったが、**checkout を先**にした。理由: git2 の checkout は衝突（ignored との衝突など）を書込前に検出して失敗するので、checkout が拒否された時点で branch が動いていない状態を保てる（ADR-0203「先に branch を動かしてから checkout する順序も採らない」）。`force` の baseline は現在の index なので順序による差は無い。

## 3. 却下した案

- **ADR-0203 の archive commit（manifest + raw subtree）と stash `S` の併用**: 復元に Kagi 専用の reader が要る。stash 形式なら `git stash apply --index` が復元器であり、Kagi が無くても戻せる。raw bytes / mode / symlink は git の blob / tree entry がそのまま保持する（CRLF 等の変換は index 経由なので git の通常の往復と同じ）。
- **`git stash push` CLI**: 保存と消去を一括で行い、stash@{n} の identity が不安定（ADR-0203 §結論）。
- **plan 時に `worktree_digest` を取る**: 保全は実行時の内容を全部取るので「plan 時と違う」こと自体は害が無い。例外は「plan が clean 前提（backup/1 なし）で実行時に dirty」で、これは preflight が拒否する。
- **ref を先に動かす**: §2。
- **ignored も削除 / 退避**: 要望（#536）どおり触らない。

## 4. 未達・次 slice

- UI（Advanced / Dangerous menu → card → 二段 confirm）は slice 2。card では `PreservesWork` / `AbandonsCommits` / `KeepsIgnored` の warning と recovery 2 コマンドを表示する。
- CLI / MCP 露出なし。
- HEAD の Sync は sparse checkout（cone／non-cone、config・patterns・skip-worktree）、dirty／uninitialized submodule、変更対象の外部 filter／LFS を plan と preflight で拒否する（2026-10-11、#1137 amendment）。非 HEAD branch の ref-only Sync は WT に触れないためこの制限を適用しない。
- 空ディレクトリは untracked 削除後に残る（git も空ディレクトリを扱わない）。
- git2 の `checkout_tree(force)` が部分適用で失敗したときの WT 状態は「Partial + 復元コマンド」として受領に残すだけで、自動 rollback はしない（ADR-0203 §Verify）。

## Amendment — 2026-10-11: special-repository refusal (#1137)

The stash-shaped backup cannot prove preservation of sparse index flags, nested
submodule work, or externally filtered worktree content. When Sync replaces HEAD's
worktree, its plan and preflight must therefore refuse before *any* backup ref,
object/index write, checkout, or branch update:

- Active sparse-checkout configuration/patterns in this worktree, or any
  skip-worktree index entry. When `core.sparseCheckout` is explicitly false,
  Git ignores leftover cone config and patterns; so does Kagi. A linked
  worktree never inherits the main worktree's common-dir patterns.
  The typed EN/JA blocker distinguishes cone, non-cone and skip-worktree-only
  detection rather than labeling a manual skip-worktree bit as sparse-checkout.
- Submodules discovered by `repo.submodules()` whose worktree is uninitialized
  or dirty (staged, unstaged, untracked, or checked out at a different commit).
  Repository `submodule.*.ignore` must not hide work from this safety check.
- Externally filtered paths in HEAD→target deltas or the staged/unstaged/untracked
  work being preserved. Resolve current, HEAD and incoming tree attributes:
  an incoming `.gitattributes` is authoritative even before it is checked out.
  LFS is detected by attributes, not git-lfs availability or pointer heuristics.
  The Sync-specific EN/JA note explains that force checkout cannot execute the
  filter and directs users to sync with Git, not to Stage/Discard.

Ordinary repositories keep the existing round-trip behavior. Ref-only Sync of a
non-HEAD branch is unchanged. The read-only detection helper is reusable by other
force-checkout writers; snapshot restore, conflict abort and directory/file
resolution do not gain the sparse/submodule guard in this change. Discard's
target-filter guard is separately implemented by #1136.
