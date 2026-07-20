# ADR-0129 Appendix: Plan 文言テンプレート棚卸し(Phase 0 成果物)

- Status: Phase 0 完了(実装前の設計資料)
- Date: 2026-07-20
- 対象: `crates/kagi-git/src/ops/*.rs` が `OperationPlan` の
  `blockers` / `warnings` / `title` / `recovery` に流し込む全文字列。
- 行番号は本 appendix 作成時点(main = PR #168 直後)のスナップショット。
  Phase 1 実装時は行番号ではなくテンプレート文字列で突き合わせること。

## 0. 数え方の規約と集計

数え方: 「サイト」= plan フィールドへ push/代入される箇所。共有ヘルパー
(`merge_dirty_warnings`、`predict_checkout_conflict` 等)のテンプレートは
**定義側で 1 回**数え、呼び出し側はサイトとして数えない。keyed エラー
(`BranchNameError` 等)の Display は別掲(§E)。`StateSummary`
(current/predicted の head/dirty)は plan 4 フィールドでないため対象外
——ただし文字列制御が絡むものは §F に記載。

| file | blockers | warnings | titles | recoveries | issue 概算 | 差の説明 |
|---|---|---|---|---|---|---|
| history.rs | 21 | 1 | 5 | 5 | 22 | 一致 |
| switch.rs | 10 | 8 | 2 | 2 | 18 | 一致 |
| cherry_revert.rs | 15 | 3 | 6 | 5 | 18 | 一致 |
| pull.rs | 8 | 7 | 4 | 3 | 14 | +1 = helper 由来 warning を push 側でも数えた分 |
| branch.rs | 9 | 4 | 4 | 4 | 13 | 一致(create の keyed 束は §E) |
| stash.rs | 10 | 3 | 5 | 5 | 12 | +1 = SSH 版 `plan_stash_drop_remote` の warning |
| push.rs | 10 | 3 | 6 | 4 | 9 | 概算は `plan_push` 系のみ。`plan_push_branch`/`plan_set_upstream` を含む実測が正 |
| merge.rs | 8 | 1 | 2 | 1 | 9 | 一致(+ 共有ヘルパー呼び出し) |
| checkout.rs | 6 | 5 | 2 | 2 | 9 | +2 = 日本語 vec-init warning ×2(§G-1)+ helper を 2 呼び出し側で計上 |
| worktree.rs | 8 | 3 | 3 | 3 | 7 | 概算はコア 2 fn のみ。`plan_create_branch_with_checkout` と keyed 経由を含む実測が正 |
| branch_cleanup.rs | 4 | 2 | 1 | 1 | 6 | 一致 |
| mod.rs (`merge_dirty_warnings`) | 0 | 3 | 0 | 0 | 3 | 一致 |
| discard.rs | 3 | 1 | 2 | 1 | —(issue 表に無し) | Phase 1 の最初の構造化対象。§B-11 |

**Phase 2 の PR 分割はこの実測を正とする**(issue の概算は grep 粒度の違い)。

## A. Common テンプレート(op 横断 → `PlanNote::Common(CommonNote)`)

英語が「同一構文 + op 句だけ差し替え」のものは 1 バリアントに統合し、
op 句は enum 引数で持つ。**構文ごと違うものは統合しない**(ADR-0129 §1)。

| # | template (EN verbatim) | 引数 | 出現 op(箇所) | バリアント案 |
|---|---|---|---|---|
| A1 | `Repository has {} conflicted file(s). Resolve conflicts before {op}.` | count, op句 | undo(`undoing a commit`)/amend(`amending`)/undo·redo(`{label}`)/tracking-checkout(`checkout`)/switch(`switching`)/cherry(`cherry-picking`)/revert(`reverting`)/pull(`pulling`)/merge(`merging`)/checkout(`switching branches`)/stash-push(`stashing`)/stash-apply·pop(`applying a stash`)/worktree(`checking out the new branch`) | `ConflictedFiles { count, before: OpPhrase }` |
| A2 | `Working tree has {} — stash or commit changes before {op}.` | parts, op句 | tracking-checkout(`checkout`)/switch(`switching`)/cherry(`cherry-picking`)/merge(`merging`) | `DirtyBlocksOp { parts: DirtyParts, before: OpPhrase }` |
| A3 | `Suggested command: git stash push -u` | — | switch ×2, mod.rs helper | `SuggestStashPush` |
| A4 | `{} untracked file(s) will remain after checkout.` | count | tracking-checkout | `UntrackedRemain { count, ctx: AfterCheckout }` |
| A5 | `{} untracked file(s) will remain after switching.` | count | switch | 〃 `ctx: AfterSwitching` |
| A6 | `{} untracked file(s) will remain after switching branches.` | count | checkout, worktree(checkout付き) | 〃 `ctx: AfterSwitchingBranches` |
| A7 | `{} untracked file(s) will remain untouched after cherry-pick.` | count | cherry | 〃 `ctx: AfterCherryPick` |
| A8 | `{} untracked file(s) will remain untouched after revert.` | count | revert | 〃 `ctx: AfterRevert` |
| A9 | `{} untracked file(s) will remain untouched unless fetched changes need the same path.` | count | pull | 〃 `ctx: PullFetchMayTouch` |
| A10 | `{} untracked file(s) will remain untouched.` | count | mod.rs helper | 〃 `ctx: Untouched` |
| A11 | `Working tree has {}. Stash or commit before {} if you want a clean rollback point.` | parts, op句 | mod.rs helper(merge 等) | `DirtyRollbackHint { parts, op }` |
| A12 | HEAD detached 系(構文が op ごとに異なるため文単位で保持):<br>`HEAD is detached. Undo commit requires HEAD to be on a branch.` / `HEAD is detached. Amend requires HEAD to be on a branch.` / `HEAD is detached. Cherry-pick is only supported when HEAD is on a branch.` / `HEAD is detached. Revert is only supported when HEAD is on a branch.` / `HEAD is detached. Pull is only supported when HEAD is on a branch.` / `HEAD is detached. Push is only supported when HEAD is on a branch.` / `HEAD is detached. Merge is only supported on a branch.` | — | undo/amend/cherry/revert/pull/push/merge | `HeadDetached { op: PlanOp }`(message_en は op 別全文テーブル) |
| A13 | HEAD unborn 系(同上):<br>`HEAD is unborn (no commits exist). There is nothing to undo.` / `… There is nothing to amend.` / `… Cannot cherry-pick onto an empty branch.` / `… Cannot revert on an empty branch.` / `… Cannot pull onto an empty branch.` / `… Cannot push an empty branch.` / `HEAD is unborn. Cannot merge into an empty branch.` | — | 同上 | `HeadUnborn { op: PlanOp }` |
| A14 | `Branch '{}' does not exist in this repository.` | name | checkout / delete-branch / worktree-open | `BranchMissing { name, in_repo: true }` |
| A15 | `Branch '{}' does not exist.` | name | rename / set-upstream | `BranchMissing { name, in_repo: false }` |
| A16 | `HEAD is not on a branch. {} requires the operation's branch to be checked out.` | label | undo/redo | `History(HeadNotOnBranch { label })`(実質 History 専用だが構文は common 系) |

`DirtyParts` = `{n} staged` / `{n} modified` を `", "` join した部分文字列
(switch/cherry/merge 等が各所でローカル構築)。**構造化後は
`{ staged: usize, modified: usize }` を持ち、表示層で組み立てる。**

## B. カテゴリ別テンプレート(blockers / warnings)

### B-1. History(`HistoryNote`)— undo / amend / undo·redo(history_move)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Commit {} is a merge commit ({} parents). Undoing merge commits is not supported in MVP.` | short, parents | blocker | `MergeCommitUnsupported { sha, parents, op: Undo }` |
| `Commit {} is a merge commit ({} parents). Amending merge commits is not supported.` | short, parents | blocker | 〃 `op: Amend` |
| `Commit {} is the root commit (no parent). There is nothing to go back to.` | short | blocker | `RootCommit { sha, op: Undo }` |
| `Commit {} is the root commit (no parent). Amending the root commit is not supported in MVP.` | short | blocker | 〃 `op: Amend` |
| `Commit {} has been pushed to the upstream tracking branch. Undoing a pushed commit would rewrite published history, which is not allowed. Use `git revert` to create an inverse commit instead.` | short | blocker | `PushedHistoryRewrite { sha, op: Undo }` |
| `Commit {} has been pushed to its upstream tracking branch. Amending published history is not allowed (ADR-0040). Create a new commit to make the correction instead.` | short | blocker | 〃 `op: Amend` |
| `Commit message must not be empty.` | — | blocker | `EmptyMessage` |
| `Nothing staged to fold into the commit. Stage changes first, or use message-only amend.` | — | blocker | `NothingStagedForAmend` |
| `Operation was on branch '{}', but the current branch is '{}'. Switch back to '{}' to {} it.` | branch, cur, branch, label小文字 | blocker | `WrongBranch { branch, current, label }` |
| `Branch '{}' has moved since this operation (now at {}, expected {}). This history entry is stale and will be skipped.` | branch, now8, exp8 | blocker | `EntryStaleBranchMoved { branch, now, expected }` |
| `Branch '{}' has no target commit.` | branch | blocker | `BranchNoTarget { branch }` |
| `Branch '{}' no longer exists.` | branch | blocker | `BranchGone { branch }` |
| `Target commit {} is no longer reachable in the object store. This history entry is stale and will be skipped.` | to8 | blocker | `EntryStaleUnreachable { sha }` |
| `You have uncommitted changes. They will be preserved verbatim; only the branch ref moves (soft reset — index and working tree untouched).` | — | warning | `SoftMovePreservesChanges` |

(amend は `checklist::checklist(repo, &status)` の blockers/warnings も extend
する——checklist 文言は別モジュールで、本棚卸しの対象外。Phase 2 の history
PR で checklist も構造化するか判断。)

### B-2. Switch / Checkout(`SwitchNote` / `CheckoutNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Local branch name is empty.` | — | blocker | `LocalNameEmpty` |
| `Local branch '{}' already exists.` | local | blocker | `LocalExists { name }` |
| `Branch name is empty.` | — | blocker | `NameEmpty` |
| `No upstream/remote branch to switch to.` | — | blocker | `NoUpstreamToSwitch` |
| `{}`(`resolve_branch_commit` / `local_branch_oid` のエラー透過 ×2) | e | blocker | `Verbatim` 継続 → Phase 2 で `GitError` 側のキー化を検討(§G-4) |
| `Local branch '{}' does not exist; it will be created tracking {}.` | name, remote | warning | `WillCreateTracking { name, remote }` |
| `Fast-forward {} commit(s) (local knowledge; re-checked after fetch).` | behind | warning | `FfLocalKnowledge { behind }` |
| `'{}' is {} commit(s) ahead of {}; switching only, not updated.` | name, ahead, remote | warning | `AheadSwitchOnly { name, ahead, remote }` |
| `'{}' has diverged from {} ({} ahead, {} behind); switching only — merge or rebase to integrate.` | name, remote, ahead, behind | warning | `DivergedSwitchOnly { … }` |
| `Branch '{}' is already the current HEAD branch.` | branch | blocker | `AlreadyCurrent { branch }`(no-op 系 §F) |
| `{} will be carried over to '{}'.` | parts, branch | warning | `DirtyCarriedOver { parts, branch }` |
| `Working tree has local changes to {} file(s) that the target also modifies: {}. Safe checkout would be refused (the conflict prevents checkout). Stash or commit these changes first.` | n, files | blocker(helper、checkout+checkout_commit 共用) | `CheckoutOverlap { count, files }` |
| `Commit is already HEAD.` | — | blocker | `CommitAlreadyHead`(no-op 系) |
| `Working tree is dirty ({}). Safe checkout may fail; stash or commit first.` | dirty_display | warning | `DirtyMayFail { display }` |
| `detached HEAD になります。新しい作業を残す場合は branch を作成してください。` | — | warning | **既に日本語**(§G-1)`DetachedHeadJa` 相当 → 正しくキー化 |
| `Create branch here を先に使うことを推奨します。` | — | warning | 〃 |

### B-3. CherryRevert(`CherryRevertNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Commit {} is a merge commit ({} parents). Cherry-picking merge commits requires explicit mainline selection, which is not supported in MVP.` | short, parents | blocker | `MergeCommitNeedsMainline { sha, parents, op: CherryPick }` |
| `Commit {} is a merge commit ({} parents). Reverting merge commits requires explicit mainline selection, which is not supported in MVP.` | short, parents | blocker | 〃 `op: Revert` |
| `Commit {} is the current HEAD commit. Nothing to cherry-pick.` | short | blocker | `NothingToCherryPickHead { sha }`(no-op 系) |
| `Cherry-pick would produce {} conflict(s): {}. Resolve divergence before cherry-picking.` | n, files | blocker | `WouldConflict { count, files, op }` |
| `Revert would produce {} conflict(s): {}. Resolve divergence before reverting.` | n, files | blocker | 〃 |
| `Cherry-picking {} would produce no changes — it appears to have been applied already.` | short | blocker | `NoChanges { sha, op: CherryPick }`(no-op 系) |
| `Reverting {} would produce no changes.` | short | blocker | 〃 `op: Revert` |
| `Commit {} is not contained in the current branch. Revert only operates on current-branch commits.` | short | blocker | `NotInCurrentBranch { sha }` |
| `Working tree has {}. Safe checkout may refuse if those files overlap the revert.` | parts | warning | `DirtyMayRefuse { parts }` |

### B-4. Pull(`PullNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Working tree has {}. Pull will proceed only if fetched changes do not touch those paths.` | parts | warning | `DirtyPullGuard { parts }` |
| `No upstream configured for branch '{}': {}. Set one with `git branch --set-upstream-to=<remote>/<branch>`.` | branch, e | blocker | `NoUpstreamWithHint { branch, err }` |
| `No upstream configured for branch '{}': {}.` | branch, e | blocker(pull-ff / push-branch 共通文言) | `NoUpstream { branch, err }` |
| `Plan-time merge prediction: the current upstream tip would conflict with HEAD. Execute is NOT blocked (fetch may change things), but be aware that if the upstream has not changed, execute will fail safely leaving the repo untouched.` | — | warning(helper) | `MergePrediction` |
| `Repository has {} conflicted file(s); this ref-only pull will not touch the working tree.` | count | warning | `ConflictedRefOnly { count }` |
| `Working tree is dirty; this ref-only pull will not touch the working tree.` | — | warning | `DirtyRefOnly` |
| `Branch '{}' is already up to date with its upstream.` | branch | blocker | `AlreadyUpToDate { branch, tail: Plain }`(no-op §F) |
| `Branch '{}' cannot be fast-forwarded to its upstream; pull it while checked out to merge.` | branch | blocker | `CannotFastForward { branch }` |
| SSH: `{branch} has diverged ({ahead} ahead, {behind} behind); the pull will create a merge commit on the remote.` | branch, ahead, behind | warning | `RemoteDiverged { … }` |
| SSH: `The remote working tree has uncommitted changes; the pull may fail or produce conflicts that must be resolved on the host.` | — | warning | `RemoteDirty` |

### B-5. Push(`PushNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Non-fast-forward pushes will fail — force is not used.` | — | warning | `NoForceUsed { punct: EmDash }` |
| `Non-fast-forward pushes will fail; force is not used.` | — | warning | 〃 `punct: Semicolon`(**文言ゆれ**、Phase 3 後に統一候補) |
| `No upstream configured for branch '{}' and no remotes exist. Add a remote with `git remote add origin <url>`.` | branch | blocker | `NoUpstreamNoRemotes { branch }` |
| `Branch '{}' is already up to date with its upstream — nothing to push.` | branch | blocker | `AlreadyUpToDate { branch, tail: NothingToPushEmDash }`(no-op §F) |
| `Branch '{}' is already up to date with its upstream; nothing to push.` | branch | blocker | 〃 `tail: NothingToPushSemicolon`(**ゆれ**) |
| `{}`(oid/remote 解決エラー透過 ×2) | e | blocker | §G-4 |
| `Upstream must be a remote branch name like origin/main.` | — | blocker | `UpstreamFormatInvalid` |
| `Remote-tracking branch '{}' is not present locally; config can still be set.` | upstream | warning | `UpstreamNotPresentLocally { upstream }` |

### B-6. Merge(`MergeNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Branch '{}' is already the current branch.` | target | blocker | `TargetIsCurrent { target }` |
| `{} is already HEAD. Nothing to merge.` | target | blocker | `TargetIsHead { target }`(no-op 系) |
| `Current branch '{}' already contains '{}'. Nothing to merge.` | cur, target | blocker | `AlreadyContains { current, target }`(no-op 系) |
| `Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode.` | n, files_label | warning | `WillConflict { count, files }` |
| `Merging '{}' would produce no changes.` | target | blocker | `NoChanges { target }`(no-op 系) |

### B-7. Stash(`StashNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Nothing to stash: working tree is already clean (no staged, modified, or untracked files).` | — | blocker | `NothingToStash`(no-op §F) |
| `{} untracked file(s) will be included in the stash (equivalent to `git stash push -u`).` | count | warning | `UntrackedIncluded { count }` |
| `{} untracked file(s) will NOT be included in the stash (include_untracked=false). They will remain in the working tree.` | count | warning | `UntrackedExcluded { count }` |
| `Stash index {} is out of range (only {} stash entr{} exist).` | index, count, y/ies | blocker(apply/pop/drop ×3) | `IndexOutOfRange { index, count }`(単複は message_en 側で処理——**JA では単複不要**、キー統合の好例) |
| `Working tree is dirty ({}) — stash apply is only allowed on a clean working tree to prevent accidental merge conflicts.` | parts | blocker | `DirtyBlocksApply { parts, op: Apply }` |
| `Working tree is dirty ({}) — stash pop is only allowed on a clean working tree to prevent accidental merge conflicts.` | parts | blocker | 〃 `op: Pop` |
| `Stash pop would produce {} conflict(s): {}. Pop is blocked to prevent losing the stash entry. Use 'Stash Apply' instead: it applies the stash without removing it, allowing you to resolve conflicts safely.` | n, files | blocker | `PopWouldConflict { count, files }` |
| SSH: `This permanently removes the stash entry on the remote host. It cannot be undone from Kagi.` | — | warning | `RemoteDropIrreversible` |

### B-8. Worktree(`WorktreeNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Repository has {} conflicted file(s). Resolve conflicts before checking out the new branch.` | count | blocker | → A1 |
| `Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing.` | parts | blocker | `DirtyBlocksCheckoutAfterCreate { parts }` |
| `Branch '{}' is already checked out in another worktree: {}` | branch, path | blocker | `BranchInOtherWorktree { branch, path }` |
| `Creates a linked worktree at '{}' with branch '{}' (start point {}).` | path, branch, start | warning | `CreatesLinkedWorktree { path, branch, start }` |
| `Locked with reason: {} — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed.` | reason_display | warning | `LockedWithReason { reason: Option<String> }`(`(no reason recorded)` は表示層で) |
| `Worktree '{}' is already unlocked.` | name | blocker | `AlreadyUnlocked { name }`(no-op 系) |
| `Could not read the lock state of worktree '{}': {}` | name, e | blocker | `LockStateUnreadable { name, err }` |
| `Worktree '{}' does not exist.` | name | blocker | `WorktreeMissing { name }` |

### B-9. Branch(`BranchNote`)— delete / rename(create は §E keyed)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Commit '{}' does not exist in this repository.` | short | blocker | `CommitMissing { sha }` |
| `Working tree is dirty; branch rename is ref-only and will not touch files.` | — | warning | `RenameRefOnlyDirty` |
| `Remote branch names are not renamed automatically; only local branch config is carried over.` | — | warning | `RenameRemoteNotRenamed` |
| `Branch '{}' is the currently checked-out branch. Checkout a different branch before deleting this one.` | name | blocker | `DeleteCurrentBranch { name }` |
| `Branch '{}' is checked out in LOCKED worktree '{}'. Unlock it first (right-click the worktree in the sidebar → Unlock worktree) before deleting the branch.` | name, path | blocker | `DeleteBranchInLockedWorktree { name, path }` |
| `Branch '{}' is checked out in worktree '{}' which has uncommitted changes. Commit or discard them there first — the worktree is not removed while it holds work.` | name, path | blocker | `DeleteBranchInDirtyWorktree { name, path }` |
| `Branch '{}' is checked out in clean worktree '{}'. The worktree will be removed, then the branch deleted.` | name, path | warning | `DeleteRemovesPinningWorktree { name, path }`(§F-3 の意味的状態も参照) |
| `HEAD is detached and points to the same commit as '{}'. This branch cannot be deleted while HEAD is at its tip.` | name | blocker | `DeleteDetachedAtTip { name }` |
| `Branch '{}' has unmerged commits (tip {} is not reachable from HEAD). Merge or discard the branch manually before deleting. Force delete is not provided.` | name, tip8 | blocker | `DeleteUnmerged { name, tip }` |
| `Branch '{}' has an upstream tracking branch. Only the local branch will be deleted; the remote branch is NOT removed.` | name | warning | `DeleteKeepsRemote { name }` |

### B-10. BranchCleanup(`CleanupNote`)

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `No branches selected for deletion.` | — | blocker | `NoSelection` |
| `Branch '{}' is no longer a cleanup candidate. Refresh the list.` | name | blocker | `NoLongerCandidate { name }` |
| `Branch '{}' is not safely deletable (it may have grown new commits since merge). Refresh the list.` | name | blocker | `NotSafelyDeletable { name }` |
| `Branch '{}' moved since the list was built. Refresh the list.` | name | blocker | `TipMoved { name }` |
| `Some branches are only *likely* squash-merged (upstream gone); there is no local proof of the merge.` | — | warning | `SquashHeuristicOnly` |
| `Remote branches on 'origin' will be deleted (network write).` | — | warning | `RemoteDeleteNetwork` |

### B-11. Discard(`DiscardNote`)— Phase 1 の最初の構造化 producer

| template (EN verbatim) | 引数 | 種別 | バリアント案 |
|---|---|---|---|
| `Nothing to discard: no files selected.` | — | blocker | `NothingSelected`(no-op 系) |
| `'{}' is conflicted. Resolve the conflict instead of discarding it.` | rel | blocker | `TargetConflicted { path }` |
| `'{}' has no unstaged changes to discard.` | rel | blocker | `NoUnstagedChanges { path }` |
| `⚠️ {} untracked file(s) will be PERMANENTLY DELETED from disk (and any now-empty folders removed). A backup blob is saved to the oplog first — recover with `git cat-file -p <blob-sha>`.` | count | warning | `UntrackedWillBeDeleted { count }` |

title: `Discard changes to '{}'`(単一)/ `Discard changes to {} file(s)`(複数)
→ `PlanTitle::Discard { first: Option<String>, count }`。
recovery: `This discards your unstaged changes to the selected file(s): tracked files are restored from the index, untracked files are deleted from disk. Either way a backup blob of each file's current content is recorded in the oplog (op="discard") first; recover with `git cat-file -p <blob-sha>`.`
→ `RecoveryKind::Discard`、commands = `git cat-file -p <blob-sha>`。

## C. Title テンプレート → `PlanTitle`

title は「1 個・必須」(ADR-0129 §1)。**op ごとに 1 バリアント + 状態引数**
とし、blocked/no-op の文言分岐(undo/amend/push)は enum 引数で表す。

| op | template(s) (EN verbatim) | バリアント案 |
|---|---|---|
| undo | `Undo commit {} '{}' — changes will be staged` / `Undo last commit (cannot proceed — see blockers)` | `UndoCommit { sha, summary, blocked: bool }` |
| amend | `Amend commit {} '{}' ({}) — SHA will change` / `Amend last commit (cannot proceed — see blockers)` | `Amend { sha, summary, mode, blocked }` |
| undo/redo | `{} {} on '{}' — {} → {}` | `HistoryMove { label, kind_slug, branch, from, to }` |
| tracking-checkout | `Checkout {} as local branch {}` | `CheckoutTracking { remote, local }` |
| switch-to-latest | `Switch to latest {} (fetch {})` | `SwitchToLatest { branch, remote }` |
| checkout | `Checkout branch '{}'` | `Checkout { branch }` |
| checkout-commit | `Checkout commit {} '{}' (detached HEAD)` | `CheckoutCommit { sha, summary }` |
| cherry-pick | `Cherry-pick {} onto {}` / `Cherry-pick {} '{}' onto {}` | `CherryPick { sha, summary: Option, branch }` |
| revert | `Revert {} '{}' on {}` | `Revert { sha, summary, branch }` |
| pull (SSH) | `Pull {branch} — up to date (local knowledge)` / `Pull {branch} from {upstream} — {behind} commit(s) behind` | `PullRemote { branch, upstream, behind }`(behind=0 で up-to-date 形) |
| pull | `Pull '{}' from '{}'  ({})`(※ `'` の後ろ空白 2 個)+ behind_label 副テンプレート `up to date (local knowledge; fetch may reveal more)` / `{} behind upstream (local knowledge; fetch may reveal more)` | `Pull { branch, remote, behind: Option<usize> }` — **§F-1 の title 判定をここで殺す** |
| pull-ff | `Pull '{}' from '{}' (ff-only, ref-only, {} behind)` | `PullBranchFf { branch, remote, behind }` |
| push | `Push '{}' to '{}' (set upstream)` / `Push '{}' to '{}'` / `Push (blocked)` | `Push { branch, remote, set_upstream, blocked }` |
| push-branch | `Push '{}' to '{}/{}' (set upstream)` / `Push '{}' to '{}'` | `PushBranch { … }` |
| set-upstream | `Set upstream of '{}' to '{}'` | `SetUpstream { branch, upstream }` |
| merge | `Merge {} into {}` / `Merge {} into current branch` | `Merge { target, into: Option<String> }` |
| stash-push | `Stash push — save local modifications ({})` | `StashPush { next_count }` |
| stash-apply | `Stash apply — restore stash@{{}}` | `StashApply { index }` |
| stash-pop | `Stash pop — apply and remove stash@{{}}` | `StashPop { index }` |
| stash-drop | `Stash drop — delete stash@{{}}` / SSH: `Drop {stash_label}` | `StashDrop { index }` / `StashDropRemote { label }` |
| create-branch | `Create branch '{}' @ {}` / `… and checkout` | `CreateBranch { name, at, checkout: bool }` |
| rename | `Rename branch '{}' to '{}'` | `RenameBranch { old, new }` |
| delete-branch | `Delete branch '{}' (tip {})` / `Delete branch '{}'` | `DeleteBranch { name, tip: Option }` |
| worktree | `Create worktree '{}' @ {}` / `Open worktree for '{}'`(上書きされ dead 疑い、§G-5) / `Unlock worktree '{}'` | `CreateWorktree { branch, start }` / `UnlockWorktree { name }` |
| cleanup | `Delete {} merged branch(es)` | `CleanupDelete { count }` |

## D. Recovery テンプレート → `PlanRecovery`

recovery は全 op で「説明文 + 具体コマンド(1〜3 個)+ 補足」という構造を
持つ(delete-branch の `lines().nth(1)` 抽出はこの構造への依存)。提案:

```rust
pub struct PlanRecovery {
    pub kind: RecoveryKind,          // op 別バリアント(引数付き)
    pub commands: Vec<String>,        // 実行可能コマンド(表示は等幅、コピー可)
}
// UI の「restore: <cmd>」footer は commands.first() を使う(nth(1) parse 廃止)
```

recovery テンプレート(kind 別、コマンドは `commands` へ分離):

| op | 本文テンプレート (EN verbatim、`\n` 含む) | commands |
|---|---|---|
| undo | `The undone commit is NOT deleted — it remains in the object store and reflog.\nTo fully restore (re-commit with the same SHA):\n  git reset --soft {}\nChanges from the undone commit will be staged immediately after undo.\nThe reflog records every HEAD movement:\n  git reflog` | `git reset --soft {sha}` / `git reflog` |
| undo(blocked) | `Undo commit cannot proceed (see blockers above).` | — |
| amend | `Amend rewrites history: the new commit gets a NEW SHA and the old commit {} becomes unreachable from the branch (but stays in the reflog).\nTo restore the original commit:\n  git reset --hard {}\nThe reflog records every HEAD movement:\n  git reflog` | `git reset --hard {sha}` / `git reflog` |
| amend(blocked) | `Amend cannot proceed (see blockers above).` | — |
| undo/redo | `{} moves branch '{}' from {} to {} via a safe ref move (no reset --hard, no clean). The {} commit is NOT deleted — it stays in the object store and reflog:\n  git reflog\nTo restore manually:\n  git update-ref refs/heads/{} {}` | `git reflog` / `git update-ref refs/heads/{branch} {from}` |
| tracking-checkout | `If checkout succeeds but you do not want the branch, switch back and delete it:\n  git checkout -\n  git branch -d {}` | `git checkout -` / `git branch -d {local}` |
| switch-to-latest | `Fetches {} then switches to {}, fast-forwarding only when safe. Diverged/ahead branches are switched to but never moved. To go back: git checkout -` | `git checkout -` |
| checkout | `If anything goes wrong you can return to '{}' with:\n  git checkout {}\nThe reflog records every HEAD movement:\n  git reflog` | `git checkout {prev}` / `git reflog` |
| checkout-commit | `If this was accidental, return with:\n  git checkout {}\nTo keep new work from the detached state, create a branch:\n  git switch -c <name>\nThe reflog records every HEAD movement:\n  git reflog` | 同様 3 個 |
| cherry-pick | `To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\nThe previous HEAD sha is recorded in the reflog:\n  git reflog`(4 サイト同一文言) | `git revert <new-commit-sha>` / `git reflog` |
| revert | `To undo this revert after execution, revert the new revert commit:\n  git revert <new-revert-commit-sha>\nThe previous HEAD sha is recorded in the reflog:\n  git reflog` | 同様 |
| pull | `Pull is non-destructive: fast-forward and clean merges do not lose work.\nDirty working-tree paths are checked against the fetched update before checkout.\nIf the merge would conflict or overwrite dirty paths, execute is blocked and the repo remains untouched.\nTo undo a merge commit after execution:\n  git reset --hard HEAD~1\nThe reflog records every HEAD movement:\n  git reflog` | `git reset --hard HEAD~1` / `git reflog` |
| pull (SSH) | `Runs `git pull` on the host using its own credentials. Conflicts are left for resolution on the host.` | — |
| pull-ff | `This updates only refs/heads/{} after verifying a fast-forward. The working tree is not changed. If needed, restore the old tip with git branch -f {} <old-sha>.` | `git branch -f {branch} <old-sha>` |
| push | `Push only sends commits to the remote — the local repository is never modified.\nIf the push is rejected (non-fast-forward), pull first and re-plan:\n  git pull\n  git push\nThe reflog records every HEAD movement:\n  git reflog` | `git pull` / `git push` / `git reflog` |
| push(blocked) | `Push requires a branch. Use `git checkout <branch>` to attach HEAD.` | `git checkout <branch>` |
| push-branch | `Push sends commits to the remote and does not modify the working tree. If the push is rejected, fetch or pull first and re-plan.` | — |
| set-upstream | `This changes only branch.{}.remote and branch.{}.merge in git config. To undo, set the previous upstream again.` | — |
| merge | `If this merge is not wanted after execution, use git reflog to find the previous HEAD.\nFast-forward merges can be undone by moving the branch back; merge commits can be reverted with git revert -m 1 <merge-commit>.` | `git reflog` / `git revert -m 1 <merge-commit>` |
| stash-push | `To inspect stash entries:  git stash list\nTo restore without removing the stash entry:  git stash apply stash@{0}\nStash message that will be used: "{}"` | 2 個 |
| stash-apply | `The stash entry stash@{…} is NOT removed by apply — it remains in the list.\nIf the apply caused conflicts, resolve them manually; the stash is safely preserved.\nTo see remaining stash entries:  git stash list\nStash message: "{}"` | `git stash list` |
| stash-pop | `WARNING: pop = apply + drop.  If apply succeeds, stash@{…} is permanently removed.\nThe stash entry "{}" will be consumed.\nTo restore without removing the stash: use 'Stash Apply' instead.\nTo see remaining stash entries:  git stash list` | `git stash list` |
| stash-drop | `Drop removes the stash entry only — the working tree is NOT touched.\nThe dropped stash commit {} stays reachable from the stash reflog until gc; restore it with:\n  git stash store -m "{}" {}\nTo see remaining stash entries:  git stash list`(oid 無し時は 1 行目のみ) | `git stash store -m "{msg}" {oid}` / `git stash list` |
| stash-drop (SSH) | `A dropped stash commit may remain reachable from the remote's stash reflog until gc, but Kagi does not manage remote recovery.` | — |
| create-branch | `The new branch '{}' can be removed without side effects:\n  git branch -d {}\n(Branch creation does not move HEAD or alter the working tree.)` | `git branch -d {name}` |
| create+checkout | `This creates branch '{}' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d {}\nTo return after checkout:\n  git checkout {}` | 2 個(※ `current.head.strip_prefix("branch: ")` 依存 → 構造化で解消、§F-9) |
| rename | `This renames only the local ref. To undo: git branch -m {} {}` | `git branch -m {new} {old}` |
| delete-branch | `To restore the deleted branch:\n  git branch {} {}\nThe branch tip commit '{}' remains in the object store until GC.`(blocked 時: `Branch '{}' could not be found. Use `git branch` to list local branches.`) | `git branch {name} {tip}` — **`lines().nth(1)` の置換先** |
| worktree | `Remove the linked worktree if needed:\n  git worktree remove {}\nThe branch can then be removed with:\n  git branch -d {}` | 2 個 |
| unlock | `Re-lock the worktree if needed:\n  git worktree lock --reason "<why>" <path-of-{}>` | 1 個 |
| cleanup | `Every deleted tip OID is recorded in the oplog. To restore:\n  git branch <name> <oid>          (local)\n  git push origin <oid>:refs/heads/<name>   (remote)` | 2 個 |

## E. 既にキー化済みのエラー(PlanNote へ吸収)

`kagi-domain/src/plan.rs` の Display 実装。移行モデルの先行事例であり、
Phase 1 で `PlanNote::Branch(...)` / `PlanNote::Worktree(...)` に取り込み、
`localize_plan_blockers` シム(§F-7)を不要化する。

- `BranchNameError` 9 変種: `Branch name must not be empty.` / `Branch name is required.` / `Branch name must not start or end with whitespace.` / `Branch already has that name.` / `Branch '{}' already exists.` / `'{}' is not a valid branch name.` / `Branch name '{}' is not a valid git ref name (no spaces, '..', or other invalid characters).` / `Branch name '{}' must not start with '-'.` / `A branch named '{}' already exists in this repository.`
- `WorktreePathError`: `Worktree path must not be empty.` / `Worktree path '{}' already exists.`
- `WorktreeValidationError::Other(s)` の English-only 透過(**未キー化**、worktree.rs 内で構築): `Repository root is not accessible: {}` / `Worktree path must have a parent directory.` / `Parent directory '{}' does not exist.` / `Parent directory is not accessible: {}` / `Worktree path must name a directory.` / `Worktree path '{}' must be outside the repository.` → Phase 2 worktree PR でキー化。

## F. 文字列制御箇所の全列挙 → `PlanDisposition` 設計

不変条件(ADR-0129 §2): no-op 判定・復旧処理・安全判定で表示文字列を
参照しない。以下が現存する全違反サイトと置換先。

### F-1〜F-2: no-op 判定(PlanDisposition の直接動機)

| # | 箇所 | 現状 | 置換 |
|---|---|---|---|
| F-1 | `src/ui/operations/pull_push.rs:85` | `plan.title.contains("up to date (local knowledge")` | `plan.disposition == NoOp(PullUpToDate)` |
| F-2 | `src/ui/operations/pull_push.rs:342` | `plan.blockers.iter().all(\|b\| b.contains("nothing to push"))` | `plan.disposition == NoOp(PushUpToDate)` |

```rust
pub enum PlanDisposition { Ready, NoOp(NoOpKind), Blocked }
pub enum NoOpKind {
    PullUpToDate, PushUpToDate, NothingToStash, NothingToMerge,
    NothingToCherryPick, NothingToRevert, AlreadyOnBranch, CommitIsHead,
    WorktreeAlreadyUnlocked, PullFfUpToDate,
}
```

producer が plan 生成時に設定する。**no-op は現状 blocker として表現されて
いる**(§B の「no-op 系」印: stash `NothingToStash`、merge 3 種、cherry 2 種、
revert 1 種、checkout `AlreadyCurrent`/`CommitAlreadyHead`、pull-ff/push の
up-to-date、unlock `AlreadyUnlocked`)。Phase 1 では blockers 表示は不変の
まま disposition を追加し、F-1/F-2 の判定だけ置換する(挙動不変)。UI 表現
の改善(no-op をブロッカー赤表示から情報表示へ)は Phase 3 以降の別件。

### F-3〜F-9: その他の文字列依存

| # | 箇所 | 現状 | 置換 |
|---|---|---|---|
| F-3 | `src/ui/operations/branch.rs:1365` | `plan.warnings.iter().any(\|w\| w.contains("worktree"))` で klog `executed: delete-branch removed pinning worktree` を出す | 型付き note `DeleteRemovesPinningWorktree` の存在判定(または plan にフラグ `removes_pinning_worktree`) |
| F-4 | `src/ui/operations/branch.rs:1287` / `:1368-1373` | `plan.recovery.lines().nth(1)` で復元コマンド抽出(recovery の 2 行目が `  git branch {} {}` である構造に依存) | `PlanRecovery.commands.first()` |
| F-5 | `src/ui/operations/checkout.rs:167` | `plan.blockers.join(" / ")` を footer 文字列に | localize 済み note を join(表示専用。構造は壊れないが Phase 1 で `plan_note_text` 経由に) |
| F-6 | `src/ui/operations/checkout.rs:558-566` | **UI が `Msg::DirtyStashFirst.t()`(ローカライズ済み文字列)を plan.warnings に挿入**——plan 内に EN/JA が混在しうる | 型付き note の挿入(`CommonNote::DirtyStashFirst`)に置換。message_en 契約の例外として Phase 1 で先に処理 |
| F-7 | `src/ui/mod.rs:834` `localize_plan_blockers` + `modals.rs:202/232` `localized_blockers`(呼び出し: `operations/branch.rs:88`, `operations/worktree.rs:132`) | EN 文字列一致 → localized 置換の部分シム(二重状態) | Phase 3 で削除。全 renderer が `plan_note_text()` 直呼び |
| F-8 | `crates/kagi-git/src/ops/merge.rs:71-74` | `warnings.retain(\|w\| !w.to_lowercase().contains("working tree has") && !w.to_lowercase().contains("suggested command: git stash"))` — **producer 内**で共有ヘルパー文言を substring 除去 | 型付き note のバリアント一致で retain(`!matches!(w, Common(DirtyRollbackHint{..}) \| Common(SuggestStashPush))`) |
| F-9 | `crates/kagi-git/src/ops/worktree.rs:156` | `plan.current.head.strip_prefix("branch: ")` で recovery 用ブランチ名抽出(StateSummary の表示形式に依存) | plan 生成時点でブランチ名を値として保持(`Head::Attached{branch}` から直接) |

### F-10: 境界・周辺(違反ではないが移行時に触る)

- **oplog 境界**: `crates/kagi-git/src/oplog.rs` `OpOutcome::Refused { blockers: Vec<String> }` — on-disk `[String]` 維持。`OperationPlan` → oplog 変換点で `message_en()`(ADR-0129 §3)。
- **cherry_revert.rs:785** `if current.dirty == "clean"` — StateSummary の
  `"clean"` トークン比較(producer 内)。StateSummary は本 ADR の対象外だが、
  同種の文字列比較として Phase 2 cherry PR で bool 化を推奨。
- **pull.rs テスト(804-819)** `plan.title.contains("3 commit")` 等 — テストが
  文言に結合。Phase 1 で構造化 assert(バリアント一致)+ message_en golden に
  分離。
- **history.rs `infer_kind_from_reflog`(1085-1095)** — git の **reflog メッセージ**
  parsing であり plan 文字列ではない(対象外。ただし「文字列 parsing」棚卸しと
  して記録)。
- **`kagi-ui-core/src/i18n.rs` の既存 EN 定数**(`AlreadyUpToDatePush`
  `Push: nothing to push (ahead=0)` `BcmNothingToPush` 等)— plan 文言と同族の
  トースト/フッター文字列。モーダル外は非目標(ADR-0129)だが、NoOpKind 導入
  時にこれらの表示分岐も disposition ベースへ寄せられる(Phase 2 push/pull PR
  の任意項目)。

## G. 特記事項(Phase 1 実装者への注意)

1. **plan 内に既に日本語がある**: checkout.rs L267/268 の 2 warning
   (`detached HEAD になります。…` / `Create branch here を先に使うことを推奨します。`)は
   vec-init で無条件挿入される日本語文字列。「EN 正本 + message_en バイト
   同一」の前提の**既存例外**。Phase 1 では Verbatim のまま包み(バイト
   同一を維持)、Phase 2 checkout PR で正式にキー化(EN 文言を新規作成)する。
   cherry_revert.rs L782 の `predicted.head` にも日本語あり(StateSummary
   なので契約対象外だが同時に整理推奨)。
2. **文言ゆれ**(統合は Phase 3 後、移行中はバイト維持): push の
   `— nothing to push.` vs `; nothing to push.`、`Non-fast-forward pushes will
   fail —` vs `fail;`、untracked 系 7 変種、`Branch '{}' does not exist(in
   this repository)?.` 2 変種。
3. **`\` 行継続**: 多数のテンプレートがソース上 `\` 継続で書かれており、
   ランタイム文字列は改行なしで連結される。golden test はランタイム文字列で
   固定すること(ソースの見た目ではなく)。
4. **エラー透過 blocker**(`{}` に `GitError`/`git2::Error` の Display を
   そのまま): switch ×2、push-branch ×2。PlanNote 化では
   `Common(GitErrorPassthrough { message: String })` として保持(GitError 自体の
   キー化は別 ADR 級のため本移行では非目標)。
5. **dead code 疑い**: worktree.rs L219 の title `Open worktree for '{}'` は
   L251 の代入で常に上書きされる。Phase 2 worktree PR で削除確認。
6. **stash の単複処理**(`entr{}` + `"y"/"ies"`)は message_en 側ロジックに
   吸収(JA は単複不要)。
7. **checklist**(amend が extend する `checklist::checklist`)は別モジュール。
   Phase 2 history PR のスコープに含めるか、その時点で判断。

## H. `kagi-ui-core::i18n::plan/` 分割案(ADR-0129 §3 の feature 分割)

| ファイル | 対応カテゴリ |
|---|---|
| `plan/common.rs` | §A(CommonNote)+ PlanDisposition 表示 |
| `plan/branch.rs` | §B-9 + §E BranchNameError |
| `plan/stash.rs` | §B-7 |
| `plan/history.rs` | §B-1 |
| `plan/remote.rs` | §B-4 Pull + §B-5 Push + SSH 系 |
| `plan/switch.rs` | §B-2(switch/checkout)+ §B-6 Merge |
| `plan/worktree.rs` | §B-8 + §E WorktreePathError |
| `plan/cleanup.rs` | §B-10 |
| `plan/title.rs` / `plan/recovery.rs` | §C / §D |
