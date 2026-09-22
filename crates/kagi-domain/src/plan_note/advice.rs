/// English advice templates shared by domain renderers and `Msg` (#353).
///
/// Literal expansion preserves `format!`'s compile-time argument checks and
/// keeps the English wording in one place, without runtime parsing.
/// Positional holes use the domain renderer's argument order. Zero-argument
/// tags are plain text: `DirtyStashFirst` contains literal `stash@{0}`.
#[macro_export]
macro_rules! advice_template_en {
    // §A4–A10 — `CommonNote::UntrackedRemain` (one tag per `UntrackedCtx`).
    (UntrackedAfterCheckout) => {
        "{} untracked file(s) will remain after checkout."
    };
    (UntrackedAfterSwitching) => {
        "{} untracked file(s) will remain after switching."
    };
    (UntrackedAfterSwitchingBranches) => {
        "{} untracked file(s) will remain after switching branches."
    };
    (UntrackedAfterCherryPick) => {
        "{} untracked file(s) will remain untouched after cherry-pick."
    };
    (UntrackedAfterRevert) => {
        "{} untracked file(s) will remain untouched after revert."
    };
    (UntrackedPullFetchMayTouch) => {
        "{} untracked file(s) will remain untouched unless fetched changes need the same path."
    };
    (UntrackedUntouched) => {
        "{} untracked file(s) will remain untouched."
    };
    // §A3 — `CommonNote::SuggestStashPush`.
    (SuggestStashPush) => {
        "Suggested command: git stash push -u"
    };
    // `PushNote::NoForceUsed` — one tag per `PushPunct`.
    (NoForceUsedEmDash) => {
        "Non-fast-forward pushes will fail — force is not used."
    };
    (NoForceUsedSemicolon) => {
        "Non-fast-forward pushes will fail; force is not used."
    };
    // `CheckoutNote` §G-1 pair.
    (WillDetachHead) => {
        "This will leave you in a detached HEAD state. \
         Create a branch first if you want to keep new work."
    };
    (RecommendCreateBranchHereFirst) => {
        "Using 'Create branch here' first is recommended."
    };
    // §F-6 — `CommonNote::DirtyStashFirst` (contains a literal `{0}`).
    (DirtyStashFirst) => {
        "Working tree is dirty: confirming will stash your \
         changes first (saved to stash@{0}, restore with `git stash pop`)"
    };
    // `PullNote::AutoStash` — the hole is the joined change summary.
    (PullAutoStash) => {
        "Working tree has {}. Kagi will stash these changes, pull, then restore them. If restoration conflicts, the stash is kept."
    };
    // `SwitchNote::DivergedSwitchOnly` — name, remote, ahead, behind.
    (DivergedSwitchOnly) => {
        "'{}' has diverged from {} ({} ahead, {} behind); switching only — \
         merge or rebase to integrate."
    };
    // `BranchNote::DeleteUnmerged` — name, tip, commits.
    (DeleteUnmerged) => {
        "Branch '{}' is unmerged (tip {}). Deleting it makes {} commits unreachable from other refs; a recovery ref will retain them. Confirm twice to delete."
    };
    // `StashNote::UntrackedExcluded` — count.
    (UntrackedExcluded) => {
        "{} untracked file(s) will NOT be included in the stash \
         (include_untracked=false). They will remain in the working tree."
    };
    (CommonBranchInvalidRef) => {
        "Branch name '{}' is not a valid git ref name (no spaces, '..', or other invalid characters)."
    };
    (CommonConflictedFiles) => {
        "Repository has {} conflicted file(s). Resolve conflicts before {}."
    };
    (CommonDirtyBlocksOp) => {
        "Working tree has {} — stash or commit changes before {}."
    };
    (CommonDirtyRollbackHint) => {
        "Working tree has {}. Stash or commit before {} if you want a clean rollback point."
    };
    (CommonPartialCloneObjectMissing) => {
        "This repository is a partial clone and the object this needs has not been fetched yet, so Kagi cannot read it ({}). Run `git fetch` in the repository to download it; `git` fetches missing objects on demand, and Kagi does not."
    };
    (CommonSparseExcludedPath) => {
        "'{}' is excluded by sparse-checkout, so it is absent from the working tree on purpose — not deleted. Staging it would record a deletion you did not make. Git refuses this too; widen the sparse-checkout definition first if you meant to change it."
    };
    (CommonMergeConflictWarning) => {
        "This merge will produce conflicts. It will leave conflict markers and enter Conflict Mode, where you resolve each file (or abort to restore the pre-merge state)."
    };
    (CommitNothingStaged) => {
        "Nothing to commit: no files are staged. Use stage_file() to stage changes before committing."
    };
    (CommitConflictedFiles) => {
        "Repository has {} conflicted file(s). Resolve all conflicts before committing."
    };
    (CommitLeftoverNotIncluded) => {
        "{} file(s) ({}) will NOT be included in this commit."
    };
    (DiscardTargetConflicted) => {
        "'{}' is conflicted. Resolve the conflict instead of discarding it."
    };
    (DiscardTargetSubmodule) => {
        "'{}' is a submodule. Discard cannot operate on submodules; manage the change from inside the submodule instead."
    };
    (DiscardUntrackedWillBeDeleted) => {
        "⚠️ {} untracked file(s) will be PERMANENTLY DELETED from disk (and any now-empty folders removed). A backup blob is saved to the oplog first — recover with `git cat-file -p <blob-sha>`."
    };
    (SnapshotSavepointFirst) => {
        "A snapshot of your current working tree is taken first, so this restore can itself be undone by restoring that savepoint."
    };
    (SnapshotRewritesWorkingTree) => {
        "This makes your working tree match the snapshot: tracked files are overwritten and the files it recorded are re-created. Your current state is saved as a savepoint first, so this is recoverable."
    };
    (RulesetPatternViolation) => {
        "{} does not satisfy the branch ruleset: {}."
    };
    (RulesetPatternUncheckable) => {
        "{} is constrained by a regex ruleset (/{}/ ) that Kagi cannot verify locally — GitHub will check it on push."
    };
    (RulesetFileTooLarge) => {
        "{} ({}) exceeds the ruleset's max file size ({}); GitHub will reject the push."
    };
    (RulesetRestrictedExtension) => {
        "{} has a restricted extension (.{}); the ruleset forbids it."
    };
    (RulesetRestrictedPath) => {
        "{} matches a restricted path pattern ({}); the ruleset forbids it."
    };
    (RulesetPathTooLong) => {
        "{} has a {}-character path, over the ruleset's limit of {}."
    };
    (RulesetSignatureRequired) => {
        "The branch ruleset requires signed commits, but commit signing is not configured (commit.gpgsign / user.signingkey)."
    };
    (RulesetLinearHistoryRequired) => {
        "The branch ruleset requires linear history; a merge commit would be rejected on push."
    };
    (RulesetNonFastForward) => {
        "The branch ruleset forbids non-fast-forward updates to this branch."
    };
    (RulesetCreationBlocked) => {
        "The branch ruleset forbids creating this branch."
    };
    (RulesetConstraintsUnknown) => {
        "The branch ruleset could not be determined; Kagi is keeping the conservative flow rather than assuming there are no rules."
    };
    (ChecklistPossibleSecretFileStaged) => {
        "Possible secret file staged: {} — confirm before committing."
    };
    (ChecklistLargeBinaryStaged) => {
        "Large binary file staged: {} ({}). Confirm before committing."
    };
    (ChecklistConflictMarkerFound) => {
        "Conflict marker found in staged file: {}. Resolve the merge conflict before committing."
    };
    (ChecklistPossibleSecretContentStaged) => {
        "Possible secret content in staged file: {} — confirm before committing."
    };
    (MergeWillConflict) => {
        "Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode."
    };
    (MergeUnrelatedHistories) => {
        "'{}' and the current branch have no common history. git refuses this without --allow-unrelated-histories; merging unrelated trees is almost always a mistake."
    };
    (MergeOperationInProgress) => {
        "{} is already in progress. Finish or abort it before merging."
    };
    (MergeUntrackedWouldBeOverwritten) => {
        "{} untracked file(s) would be overwritten by merge: {}. Move or remove them first."
    };
    (MergeIntoUnrelatedHistories) => {
        "'{}' and '{}' have no common history. git refuses this without --allow-unrelated-histories; merging unrelated trees is almost always a mistake."
    };
    (MergeIntoCheckedOutElsewhere) => {
        "Branch '{}' is checked out in the worktree '{}'. Merging into it from here would move its ref out from under that worktree, leaving its files and index describing a different commit. Merge from that worktree instead."
    };
    (MergeIntoWouldConflict) => {
        "Merging '{}' into '{}' conflicts in {} file(s). Conflicts are resolved in the working tree, so this one needs '{}' checked out first."
    };
    (MergeIntoFastForward) => {
        "'{}' has no commits of its own, so it fast-forwards to '{}'. Its ref moves; no merge commit is written."
    };
    (MergeIntoWorkingTreeUntouched) => {
        "Your working tree is not touched: '{}' stays checked out, and no file on disk changes."
    };
    (MergeIntoCreatesLocalBranch) => {
        "There is no local '{}' yet, so one is created at '{}' and the merge lands on it. Nothing is pushed; '{}' on the remote is unchanged."
    };
    (MergeIntoRemoteSource) => {
        "Remote-tracking ref '{}' at {} reflects the last fetch and may be stale. No fetch is performed; no local source branch is created."
    };
    (MergeIntoLocalDiffersFromRemote) => {
        "Local '{}' is not at '{}'. The merge lands on your local branch; the remote ref is not read or written."
    };
    (RebaseDirtyWorkingTree) => {
        "Working tree has uncommitted changes. Commit, stash, or discard them before rebasing."
    };
    (RebaseMayConflict) => {
        "Rebase may stop partway through with a conflict. Resolve each conflicted commit in the conflict editor, then Continue; the sequence keeps replaying until it finishes."
    };
    (CherryRevertWouldConflictCherryPick) => {
        "Cherry-pick would produce {} conflict(s): {}. Resolve divergence before cherry-picking."
    };
    (CherryRevertWouldConflictRevert) => {
        "Revert would produce {} conflict(s): {}. Resolve divergence before reverting."
    };
    (CherryRevertDirtyMayRefuse) => {
        "Working tree has {}. Safe checkout may refuse if those files overlap the revert."
    };
    (ResetRefOnlySoftReset) => {
        "This only moves the branch pointer (like `git reset --soft`): the working tree and staged changes are left exactly as they are, so this will show up as a large diff against the new HEAD, not lost files."
    };
    (ResetAbandonsCommits) => {
        "{} commit(s) will no longer be reachable from '{}' (still recoverable via reflog until GC)."
    };
    (ResetTargetNotAncestor) => {
        "The target commit is not an ancestor of '{}'. This reassigns the branch to unrelated history rather than moving it back along its own line."
    };
    (HistoryPushedHistoryRewriteUndo) => {
        "Commit {} has been pushed to the upstream tracking branch. Undoing a pushed commit would rewrite published history, which is not allowed. Use `git revert` to create an inverse commit instead."
    };
    (HistoryPushedHistoryRewriteAmend) => {
        "Commit {} has been pushed, and this branch is one other people build on. Rewriting its history would strand every clone that has already fetched it, so amend is refused here regardless of confirmation. Create a new commit to make the correction instead."
    };
    (HistoryAmendDivergesFromRemote) => {
        "Commit {} is already on the remote. Amending replaces it with a new commit, so '{}' will diverge from its upstream and a plain push will be refused. Publish the result with 'Force-with-lease push...' from the branch menu, which fails if anyone else has pushed since your last fetch."
    };
    (HistoryNothingStagedForAmend) => {
        "Nothing staged to fold into the commit. Stage changes first, or use message-only amend."
    };
    (HistoryWrongBranch) => {
        "Operation was on branch '{}', but the current branch is '{}'. Switch back to '{}' to {} it."
    };
    (HistoryHeadNotOnBranch) => {
        "HEAD is not on a branch. {} requires the operation's branch to be checked out."
    };
    (HistoryEntryStaleBranchMoved) => {
        "Branch '{}' has moved since this operation (now at {}, expected {}). This history entry is stale and will be skipped."
    };
    (HistoryEntryStaleUnreachable) => {
        "Target commit {} is no longer reachable in the object store. This history entry is stale and will be skipped."
    };
    (HistorySoftMovePreservesChanges) => {
        "You have uncommitted changes. They will be preserved verbatim; only the branch ref moves (soft reset — index and working tree untouched)."
    };
    (ConflictsObservationChanged) => {
        "conflict changed since it was observed — re-open the conflict"
    };
    (ConflictsPlanChanged) => {
        "conflict changed since planning; no files were modified"
    };
    (ConflictsRepositoryIdentityChanged) => {
        "repository identity changed after planning"
    };
    (ConflictsConflictGone) => {
        "the conflict is no longer present"
    };
    (ConflictsResolutionMarkers) => {
        "conflict markers remain in the resolution buffer"
    };
    (ConflictsUnresolvedFiles) => {
        "{} file(s) still unresolved: {}. Resolve every file before continuing."
    };
    (ConflictsMarkerResidue) => {
        "Conflict marker(s) remain in: {}. Remove all <<<<<<< ======= >>>>>>> markers before continuing."
    };
    (ConflictsIndexUnmerged) => {
        "The index still has unmerged entries not tracked by this session: {}. Re-scan the repository."
    };
    (ConflictsBinaryUnresolved) => {
        "Binary conflict(s) still need a side chosen: {}."
    };
    (ConflictsDeletionUndecided) => {
        "Keep-or-delete decision still pending for: {}."
    };
    (ConflictsEmptyMergeMessage) => {
        "The merge commit message is empty. Provide a commit message before continuing."
    };
    (ConflictsNoConflictingFilesDetected) => {
        "No conflicting files detected; continue will finish the operation as-is."
    };
    (ConflictsPartialResolutionsPreserved) => {
        "Your partial resolutions are preserved in the autosave directory and referenced in the operation log; they are not discarded."
    };
    (ConflictsSkipDiscardsStep) => {
        "Skip discards the current step's changes (the conflicting pick is dropped, not committed). Your partial resolution is preserved in the autosave directory."
    };
    (BranchRenameRefOnlyDirty) => {
        "Working tree is dirty; branch rename is ref-only and will not touch files."
    };
    (BranchRenameRemoteNotRenamed) => {
        "Remote branch names are not renamed automatically; only local branch config is carried over."
    };
    (BranchDeleteCurrentBranch) => {
        "Branch '{}' is the currently checked-out branch. Checkout a different branch before deleting this one."
    };
    (BranchDeleteBranchCheckedOut) => {
        "Branch '{}' is checked out in worktree '{}'. Switch that worktree to another branch before deleting."
    };
    (BranchDeleteBranchInLockedWorktree) => {
        "Branch '{}' is checked out in LOCKED worktree '{}'. Unlock it first (right-click the worktree in the sidebar \u{2192} Unlock worktree) before deleting the branch."
    };
    (BranchDeleteBranchInDirtyWorktree) => {
        "Branch '{}' is checked out in worktree '{}' which has uncommitted changes. Commit or discard them there first — the worktree is not removed while it holds work."
    };
    (BranchDeleteRemovesPinningWorktree) => {
        "Branch '{}' is checked out in clean worktree '{}'. The worktree will be removed, then the branch deleted."
    };
    (BranchDeleteSquashMerged) => {
        "Branch '{}' was squash-merged as {}: its commits are not ancestors of HEAD (the graph shows it as a dead end), but the identical change is already there. Nothing is lost by deleting it."
    };
    (BranchDeleteKeepsRemote) => {
        "Branch '{}' has an upstream tracking branch. Only the local branch will be deleted; the remote branch is NOT removed."
    };
    (CheckoutCheckoutOverlap) => {
        "Working tree has local changes to {} file(s) that the target also modifies: {}. Safe checkout would be refused (the conflict prevents checkout). Stash or commit these changes first."
    };
    (CheckoutDirtyCarriedOver) => {
        "{} will be carried over to '{}'."
    };
    (CheckoutDirtyMayFail) => {
        "Working tree is dirty ({}). Safe checkout may fail; stash or commit first."
    };
    (SwitchWillCreateTracking) => {
        "Local branch '{}' does not exist; it will be created tracking {}."
    };
    (SwitchFfLocalKnowledge) => {
        "Fast-forward {} commit(s) (local knowledge; re-checked after fetch)."
    };
    (SwitchAheadSwitchOnly) => {
        "'{}' is {} commit(s) ahead of {}; switching only, not updated."
    };
    (WorktreeDirtyBlocksCheckoutAfterCreate) => {
        "Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing."
    };
    (WorktreeCreatesLinkedWorktree) => {
        "Creates a linked worktree at '{}' with branch '{}' (start point {})."
    };
    (WorktreeLockedWithReason) => {
        "Locked with reason: {} — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed."
    };
    (WorktreeIncludeCopy) => {
        "Copies {} .worktreeinclude file(s) ({}) into the new worktree: {}."
    };
    (WorktreeIncludeSkippedSymlinks) => {
        "Skips {} matched symlink(s) — symlinks are not copied."
    };
    (WorktreeIncludeOverCap) => {
        ".worktreeinclude matches {}, over the {} copy cap — copy still proceeds but may be large (e.g. a node_modules match)."
    };
    (WorktreeRemoveDirty) => {
        "Worktree '{}' has uncommitted changes ({}) — commit or stash them first (removal never forces)."
    };
    (WorktreeRemoveLocked) => {
        "Worktree '{}' is locked ({}) — unlock it before removing (kagi never forces)."
    };
    (WorktreeRemovesWorktreeDeleteBranch) => {
        "Removes the linked worktree at '{}' and also deletes its branch '{}'."
    };
    (WorktreeRemovesWorktreeKeepBranch) => {
        "Removes the linked worktree at '{}' — branch '{}' is kept."
    };
    (WorktreeLocksWorktree) => {
        "Locks the worktree at '{}' with reason: {}."
    };
    (WorktreePrunePreview) => {
        "Prunes {} stale worktree admin entry(ies) whose working directory is gone: {}."
    };
    (WorktreeRepairsWorktrees) => {
        "Repairs worktree administrative links: fixes a moved main worktree, a moved linked worktree, or both. This never touches your files — only the .git links."
    };
    (WorktreePostCreateSteps) => {
        "Runs {} post-create step(s) from .kagi/worktree.toml:{}"
    };
    (WorktreePostCreateStepsTrustRequired) => {
        "Runs {} post-create step(s) from .kagi/worktree.toml:{}\n  ⚠ Confirming TRUSTS this config to run the command step(s) above (committed config is untrusted by default)."
    };
    (WorktreePreRemoveSteps) => {
        "Runs {} pre-remove step(s) from .kagi/worktree.toml (a failed or untrusted command aborts the removal):{}"
    };
    (WorktreePreRemoveStepsTrustRequired) => {
        "Runs {} pre-remove step(s) from .kagi/worktree.toml (a failed or untrusted command aborts the removal):{}\n  ⚠ Confirming TRUSTS this config to run the command step(s) above (committed config is untrusted by default)."
    };
    (TagNameErrorEmpty) => {
        "Tag name cannot be empty."
    };
    (TagNameErrorLeadingDash) => {
        "Tag name '{}' starts with '-', which is ambiguous on the command line."
    };
    (TagPushRemoteSideEffect) => {
        "This publishes tag '{}' to '{}'. Unlike every other tag action in kagi, it leaves this machine and others will see it."
    };
    (TagPushRejectedIfMoved) => {
        "If '{}' already exists on the remote pointing at a different commit, the remote rejects the push rather than moving it. kagi never force-pushes a tag."
    };
    (CleanupNoLongerCandidate) => {
        "Branch '{}' is no longer a cleanup candidate. Refresh the list."
    };
    (CleanupNotSafelyDeletable) => {
        "Branch '{}' is not safely deletable (it may have grown new commits since merge). Refresh the list."
    };
    (CleanupTipMoved) => {
        "Branch '{}' moved since the list was built. Refresh the list."
    };
    (CleanupSquashHeuristicOnly) => {
        "Some branches are only *likely* squash-merged (upstream gone); there is no local proof of the merge."
    };
    (CleanupRemoteDeleteNetwork) => {
        "Remote branches on 'origin' will be deleted (network write)."
    };
    (PushNoUpstreamNoRemotes) => {
        "No upstream configured for branch '{}' and no remotes exist. Add a remote with `git remote add origin <url>`."
    };
    (PushUpstreamFormatInvalid) => {
        "Upstream must be a remote branch name like origin/main."
    };
    (PushUpstreamNotPresentLocally) => {
        "Remote-tracking branch '{}' is not present locally; config can still be set."
    };
    (PullDirtyPullGuard) => {
        "Working tree has {}. Pull will proceed only if fetched changes do not touch those paths."
    };
    (PullNoUpstreamWithHint) => {
        "No upstream configured for branch '{}': {}. Set one with `git branch --set-upstream-to=<remote>/<branch>`."
    };
    (PullMergePrediction) => {
        "Plan-time merge prediction: the current upstream tip would conflict with HEAD. Execute is NOT blocked (fetch may change things), but be aware that if the upstream has not changed, execute will fail safely leaving the repo untouched."
    };
    (PullRestoreConflict) => {
        "Restoring the stash after this pull will conflict. Kagi merged your edit with the incoming change and these paths do not merge:{}\nCommit or stash those paths yourself first, or resolve the conflict after the pull — the stash is kept either way."
    };
    (PullRestoreConflictPossible) => {
        "Restoring the stash after this pull may conflict. Both sides changed these paths and Kagi could not merge them in advance (binary content, a mode change, or a file added or removed on one side):{}\nThe restore is attempted anyway; if it conflicts, the stash is kept."
    };
    (PullRestoreConflictMore) => {
        "\n  - … and {} more"
    };
    (PullConflictedRefOnly) => {
        "Repository has {} conflicted file(s); this ref-only pull will not touch the working tree."
    };
    (PullDirtyRefOnly) => {
        "Working tree is dirty; this ref-only pull will not touch the working tree."
    };
    (PullCannotFastForward) => {
        "Branch '{}' cannot be fast-forwarded to its upstream; pull it while checked out to merge."
    };
    (PullRemoteDiverged) => {
        "{} has diverged ({} ahead, {} behind); the pull will create a merge commit on the remote."
    };
    (PullRemoteDirty) => {
        "The remote working tree has uncommitted changes; the pull may fail or produce conflicts that must be resolved on the host."
    };
    (StashUntrackedIncluded) => {
        "{} untracked file(s) will be included in the stash (equivalent to `git stash push -u`)."
    };
    (StashDirtyBlocksApply) => {
        "Working tree is dirty ({}) — stash {} is only allowed on a clean working tree to prevent accidental merge conflicts."
    };
    (StashConflictUnknownFiles) => {
        "(unknown files)"
    };
    (StashPopWouldConflict) => {
        "Stash pop will conflict in {} file(s): {}. The stash entry will be KEPT — resolve the conflicts, then drop the stash manually."
    };
    (StashApplyWouldConflict) => {
        "Stash apply will conflict in {} file(s): {}. The stash entry stays in the list — resolve the conflicts in the working tree."
    };
    (StashApplyPredictionUnavailable) => {
        "Could not verify whether the stash applies cleanly ({}). Apply keeps the stash entry, so it is safe to try — if it conflicts, resolve the conflicts and the stash remains."
    };
    (StashPopPredictionUnavailable) => {
        "Could not verify whether the stash applies cleanly ({}). Pop is blocked because it deletes the stash entry. Use 'Stash Apply' instead: it applies the stash without removing it."
    };
    (StashRemoteDropIrreversible) => {
        "This permanently removes the stash entry on the remote host. It cannot be undone from Kagi."
    };
    (StashTargetChanged) => {
        "stash@{{{}}} is no longer the approved entry {}: another stash now occupies that position. Nothing was changed — re-plan before proceeding."
    };
    (StashListChanged) => {
        "The stash list changed since planning (order or entries differ). Nothing was changed — re-plan before proceeding."
    };
    (RemoteBranchLocalBranchUntouched) => {
        "This only deletes the branch on the remote. Your local branch '{}' is untouched and will show as having no upstream."
    };
    (ForceLeaseNoUpstream) => {
        "Branch '{}' has no upstream configured. Force-with-lease needs a known remote tip to lease-check against."
    };
    (ForceLeaseRewritesRemoteHistory) => {
        "This overwrites the remote branch '{}''s history. Anyone who already pulled the old history will need to reconcile (e.g. rebase onto the new tip)."
    };
    (ForceLeaseLeaseValue) => {
        "Protected by lease: the push is rejected if '{}' has moved past {} since your last fetch (i.e. if someone else pushed in the meantime)."
    };
    (GithubHeadUnavailable) => {
        "The head commit for #{} is unavailable. Refresh the pull-request list before merging."
    };
    (GithubNotMergeable) => {
        "GitHub reports #{} as not mergeable. Resolve conflicts (or satisfy branch protection) first."
    };
    (GithubIsDraft) => {
        "#{} is a draft. Mark it ready for review before merging."
    };
    (GithubChecksFailing) => {
        "#{} has {} failing check(s). Merging now lands code its CI rejected."
    };
    (GithubChecksPending) => {
        "#{}'s checks have not finished yet."
    };
    (GithubChangesRequested) => {
        "A reviewer requested changes on #{}."
    };
    (GithubRemoteSideEffect) => {
        "This merges on GitHub. Your local clone is unchanged until the next fetch."
    };
    (GithubDeletesBranch) => {
        "The head branch '{}' will be deleted on the remote."
    };
    (GithubForkKeepsRemoteBranch) => {
        "The remote branch in the fork is not deleted by gh."
    };
    (GithubDeletesLocalBranchPresent) => {
        "Once GitHub confirms the merge, the local branch '{}' at {} is deleted here — only if it still points there and is checked out nowhere."
    };
    (GithubDeletesLocalBranchAbsent) => {
        "The local branch '{}' does not exist here, so nothing local is deleted after the merge."
    };
    (GithubKeepsLocalBranch) => {
        "The local branch '{}' is kept, not deleted: {}"
    };
    (GithubLocalBranchKept) => {
        "local branch kept: {} ({})"
    };
    (GithubLocalBranchNotDeleted) => {
        "local branch not deleted: {}"
    };
    (GithubSuggestionRangeGone) => {
        "The lines '{}' was reviewed at no longer exist. Re-open the review against the current file."
    };
    (GithubSuggestionStale) => {
        "'{}' has changed since this suggestion was reviewed. Applying it now could edit the wrong lines, so it is refused. Re-open the review against the current file."
    };
    (GithubSuggestionWorkingTreeOnly) => {
        "This edits the working tree only — nothing is committed. Review it with hunk staging before you commit."
    };
    (GithubCommentBodyEmpty) => {
        "The comment is empty. Write something before posting it."
    };
    (GithubIssueTitleEmpty) => {
        "The issue title is empty. Write a title or add meaningful text to the body."
    };
    (GithubReviewBodyEmpty) => {
        "GitHub requires a comment on a '{}' review. Write what you want changed before submitting it."
    };
    (GithubFieldEditEmpty) => {
        "Nothing on #{} would change. Pick a reviewer, assignee or label to add or remove first."
    };
    (GithubLocalBranchNotDeletedDeletionUnauthorized) => {
        "gh reported an error, so the transport did not authorize deleting the local branch: {}"
    };
}
