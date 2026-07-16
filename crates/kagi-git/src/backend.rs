use std::path::{Path, PathBuf};

use git2::Repository;
use kagi_domain::history::HistoryEntry;
use kagi_domain::operation::{Operation, OperationOutcome};

use super::{
    conflicts, diff, diffstat, file_history, hotspot, message_gen, ops,
    resolution::ResolutionBuffer, resolve_head, snapshot, staging, status, AmendMode, AmendOutcome,
    BranchRenameValidation, CommitId, CommitPreview, DiscardOutcome, FetchOutcome, FileDiff,
    FileDiffStat, FileHistory, FileHistoryRequest, FileStatus, GitError, Head, MergeKind,
    OperationPlan, PullOutcome, PushOutcome, RawEcosystem, RepoSnapshot, UndoOutcome,
    WorkingTreeStatus,
};

pub struct Backend {
    repo: Repository,
    path: PathBuf,
}

impl Backend {
    /// Open the repository at `path`.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let path_str = path.display().to_string();

        if !path.exists() {
            return Err(GitError::PathNotFound(path_str));
        }

        let repo = Repository::open(path).map_err(|e| {
            use git2::ErrorCode;
            match e.code() {
                ErrorCode::NotFound => GitError::NotARepository(path_str.clone()),
                _ => GitError::Other(e.message().to_string()),
            }
        })?;

        if repo.is_bare() {
            return Err(GitError::BareRepository(path_str));
        }

        let path = repo
            .workdir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());

        Ok(Self { repo, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn head_state(&self) -> Result<Head, GitError> {
        resolve_head(&self.repo)
    }

    pub fn head_commit_id(&self) -> Option<CommitId> {
        self.repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| CommitId(oid.to_string()))
    }

    pub fn head_shorthand(&self) -> Option<String> {
        self.repo
            .head()
            .ok()
            .and_then(|head| head.shorthand().ok().map(str::to_string))
    }

    pub fn is_ancestor_of_head(&self, target: &CommitId) -> Result<bool, GitError> {
        let head_oid = self
            .repo
            .head()
            .map_err(|e| GitError::Other(e.message().to_string()))?
            .target()
            .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
        let target_oid =
            git2::Oid::from_str(&target.0).map_err(|e| GitError::Other(e.message().to_string()))?;
        Ok(head_oid == target_oid
            || self
                .repo
                .graph_descendant_of(head_oid, target_oid)
                .unwrap_or(false))
    }

    pub fn workdir(&self) -> Option<PathBuf> {
        self.repo.workdir().map(Path::to_path_buf)
    }

    pub fn remote_urls(&self) -> Result<Vec<String>, GitError> {
        let remotes = self
            .repo
            .remotes()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let mut urls = Vec::new();
        for name in remotes.iter().flatten().flatten() {
            if let Ok(remote) = self.repo.find_remote(name) {
                if let Ok(url) = remote.url() {
                    urls.push(url.to_string());
                }
            }
        }
        Ok(urls)
    }

    pub fn local_branch_exists(&self, name: &str) -> bool {
        self.repo.find_branch(name, git2::BranchType::Local).is_ok()
    }

    pub fn collect_staged_files(&self) -> Vec<FileStatus> {
        message_gen::collect_staged_files(&self.repo)
    }

    pub fn collect_staged_diff(&self) -> String {
        message_gen::collect_staged_diff(&self.repo)
    }

    pub fn stash_count(&mut self) -> Result<usize, GitError> {
        let mut count = 0usize;
        self.repo
            .stash_foreach(|_, _, _| {
                count += 1;
                true
            })
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Ok(count)
    }

    pub fn snapshot(&mut self, commit_limit: usize) -> Result<RepoSnapshot, GitError> {
        snapshot::snapshot(&mut self.repo, commit_limit)
    }

    pub fn working_tree_status(&self) -> Result<WorkingTreeStatus, GitError> {
        status::working_tree_status(&self.repo)
    }

    /// All tracked + untracked (non-ignored) files in the working tree,
    /// sorted, repo-relative (T-WS-EDITOR-004 Editor Workspace "All files"
    /// tree source).
    pub fn worktree_files(&self) -> Result<Vec<PathBuf>, GitError> {
        status::worktree_files(&self.repo)
    }

    pub fn commit_changed_files(&self, id: &CommitId) -> Result<Vec<FileStatus>, GitError> {
        diff::commit_changed_files(&self.repo, id)
    }

    pub fn commit_file_diff(&self, id: &CommitId, path: &Path) -> Result<FileDiff, GitError> {
        diff::commit_file_diff(&self.repo, id, path)
    }

    /// Raw blob bytes for `path` in the tree of commit `id`.
    ///
    /// `Ok(None)` when the path is absent from that tree (added later /
    /// deleted earlier). Used by the diff view's image preview (W-IMG).
    pub fn blob_bytes_at(&self, id: &CommitId, path: &Path) -> Result<Option<Vec<u8>>, GitError> {
        let oid =
            git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
        let commit = self
            .repo
            .find_commit(oid)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let tree = commit
            .tree()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let entry = match tree.get_path(path) {
            Ok(e) => e,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(GitError::Other(e.message().to_string())),
        };
        let blob = self
            .repo
            .find_blob(entry.id())
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Ok(Some(blob.content().to_vec()))
    }

    /// First parent of `id`, if any (image preview: the "before" side).
    pub fn first_parent(&self, id: &CommitId) -> Result<Option<CommitId>, GitError> {
        let oid =
            git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
        let commit = self
            .repo
            .find_commit(oid)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Ok(commit.parent_id(0).ok().map(|p| CommitId(p.to_string())))
    }

    /// Raw blob bytes for the staged (index) version of `path`, if present.
    pub fn blob_bytes_index(&self, path: &Path) -> Result<Option<Vec<u8>>, GitError> {
        let index = self
            .repo
            .index()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let Some(entry) = index.get_path(path, 0) else {
            return Ok(None);
        };
        let blob = self
            .repo
            .find_blob(entry.id)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Ok(Some(blob.content().to_vec()))
    }

    /// Raw blob bytes for `path` at HEAD, if present.
    pub fn blob_bytes_head(&self, path: &Path) -> Result<Option<Vec<u8>>, GitError> {
        let head = self
            .repo
            .head()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let commit = head
            .peel_to_commit()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let id = CommitId(commit.id().to_string());
        self.blob_bytes_at(&id, path)
    }

    pub fn compare_commits(&self, a: &CommitId, b: &CommitId) -> Result<Vec<FileStatus>, GitError> {
        diff::compare_commits(&self.repo, a, b)
    }

    pub fn compare_file_diff(
        &self,
        a: &CommitId,
        b: &CommitId,
        path: &Path,
    ) -> Result<FileDiff, GitError> {
        diff::compare_file_diff(&self.repo, a, b, path)
    }

    pub fn compare_commit_to_workdir(&self, a: &CommitId) -> Result<Vec<FileStatus>, GitError> {
        diff::compare_commit_to_workdir(&self.repo, a)
    }

    pub fn compare_commit_to_workdir_file_diff(
        &self,
        a: &CommitId,
        path: &Path,
    ) -> Result<FileDiff, GitError> {
        diff::compare_commit_to_workdir_file_diff(&self.repo, a, path)
    }

    pub fn commit_diffstat(&self, id: &CommitId) -> Result<Vec<FileDiffStat>, GitError> {
        diffstat::commit_diffstat(&self.repo, id)
    }

    pub fn staged_diffstat(&self) -> Result<Vec<FileDiffStat>, GitError> {
        diffstat::staged_diffstat(&self.repo)
    }

    pub fn unstaged_diffstat(&self) -> Result<Vec<FileDiffStat>, GitError> {
        diffstat::unstaged_diffstat(&self.repo)
    }

    pub fn stage_file(&self, path: &Path) -> Result<(), GitError> {
        staging::stage_file(&self.repo, path)
    }

    pub fn unstage_file(&self, path: &Path) -> Result<(), GitError> {
        staging::unstage_file(&self.repo, path)
    }

    pub fn stage_files(&self, paths: &[PathBuf]) -> Result<usize, GitError> {
        staging::stage_files(&self.repo, paths)
    }

    pub fn unstage_files(&self, paths: &[PathBuf]) -> Result<usize, GitError> {
        staging::unstage_files(&self.repo, paths)
    }

    pub fn unstaged_file_diff(&self, path: &Path) -> Result<FileDiff, GitError> {
        staging::unstaged_file_diff(&self.repo, path)
    }

    pub fn staged_file_diff(&self, path: &Path) -> Result<FileDiff, GitError> {
        staging::staged_file_diff(&self.repo, path)
    }

    /// Collect the change history of a single file (ADR-0089).
    ///
    /// Backed by the `git` CLI (`file_history::file_history`); does not use
    /// `self.repo`, but lives on [`Backend`] so the UI can call it uniformly.
    pub fn file_history(&self, req: &FileHistoryRequest) -> Result<FileHistory, GitError> {
        file_history::file_history(req)
    }

    /// Mine the whole repository into a [`RawEcosystem`] for the Code Ecosystem
    /// / hot-spot view (ADR-0119). Read-only; backed by the `git` CLI
    /// (`hotspot::repo_ecosystem`), not `self.repo`. `limit` caps the number of
    /// commits scanned (`0` = unlimited).
    pub fn ecosystem(
        &self,
        limit: usize,
        ignore_patterns: Vec<String>,
    ) -> Result<RawEcosystem, GitError> {
        let repo_dir = self.workdir().unwrap_or_else(|| self.path.to_path_buf());
        hotspot::repo_ecosystem(&hotspot::EcosystemRequest {
            repo_dir,
            limit,
            ignore_patterns,
        })
    }

    pub fn commit_preview(&self) -> Result<CommitPreview, GitError> {
        staging::commit_preview(&self.repo)
    }

    /// [`Self::commit_preview`] reusing an already-computed status (avoids a
    /// second `working_tree_status` walk).
    pub fn commit_preview_from_status(
        &self,
        status: &WorkingTreeStatus,
    ) -> Result<CommitPreview, GitError> {
        staging::commit_preview_from_status(&self.repo, status)
    }

    pub fn plan(&self, op: &Operation) -> Result<OperationPlan, GitError> {
        match op {
            Operation::Commit { message } => self.plan_commit(message),
            Operation::MergeCommit { message } => self.plan_merge_commit(message),
            Operation::Checkout { branch } => self.plan_checkout(branch),
            Operation::CheckoutCommit { id } => self.plan_checkout_commit(id),
            Operation::CreateBranch { name, at } => self.plan_create_branch(name, at),
            Operation::CreateBranchWithCheckout {
                name,
                at,
                checkout_after,
            } => self.plan_create_branch_with_checkout(name, at, *checkout_after),
            Operation::CreateWorktree {
                branch,
                path,
                start,
            } => self.plan_create_worktree(branch, path.as_str(), start),
            Operation::OpenWorktreeForBranch { branch, path } => {
                self.plan_open_worktree_for_branch(branch, path.as_str())
            }
            Operation::StashPush {
                message,
                include_untracked,
            } => {
                let mut backend = Backend::open(&self.path)?;
                backend.plan_stash_push(message.as_deref(), *include_untracked)
            }
            Operation::StashApply { index } => {
                let mut backend = Backend::open(&self.path)?;
                backend.plan_stash_apply(*index)
            }
            Operation::StashPop { index } => {
                let mut backend = Backend::open(&self.path)?;
                backend.plan_stash_pop(*index)
            }
            Operation::CherryPick { id } => self.plan_cherry_pick(id),
            Operation::MergeBranch { target } => {
                self.plan_merge_branch(target).map(|(plan, _)| plan)
            }
            Operation::MergeIntoConflict { target } => {
                self.plan_merge_branch(target).map(|(plan, _)| plan)
            }
            Operation::CheckoutTrackingBranch {
                remote_branch,
                local_branch,
            } => self.plan_checkout_tracking_branch(remote_branch, local_branch),
            Operation::SwitchToLatestBranch {
                branch_name,
                remote_branch,
            } => self.plan_switch_to_latest(branch_name, remote_branch),
            Operation::Revert { id } => self.plan_revert(id),
            Operation::Pull => self.plan_pull(),
            Operation::Push => self.plan_push(),
            Operation::PullBranchFf { branch_name } => self.plan_pull_branch_ff(branch_name),
            Operation::PushBranch {
                branch_name,
                set_upstream,
            } => self.plan_push_branch(branch_name, *set_upstream),
            Operation::SetUpstream {
                branch_name,
                upstream,
            } => self.plan_set_upstream(branch_name, upstream),
            Operation::RenameBranch { old_name, new_name } => {
                self.plan_rename_branch(old_name, new_name)
            }
            Operation::UndoCommit => self.plan_undo_commit(),
            Operation::Amend { mode, message } => self.plan_amend(*mode, message.as_deref()),
            Operation::DeleteBranch { name } => self.plan_delete_branch(name),
            Operation::Discard { paths } => self.plan_discard(paths),
        }
    }

    /// The single enforced entry point for every mutating operation.
    ///
    /// Implements the product's central safety invariant
    /// (`plan → confirm → preflight → execute → verify` — ADR-0104):
    /// **every** caller that mutates the repository MUST go through `run`,
    /// so the preflight check (HEAD / stash-count unchanged since the plan
    /// was built) cannot be bypassed. The oplog/toast/footer recording stays
    /// with the UI's `record_op`; `run`'s job is the safety gate + dispatch.
    ///
    /// `plan` must be a plan previously built via `plan(op)` and confirmed by
    /// the user (the confirm modal is the UI's responsibility). Operations
    /// whose plan also captures a stash count (StashApply/Pop/Drop) additionally
    /// pass through `preflight_check_stash` so a concurrent stash push between
    /// plan and execute cannot shift indices.
    ///
    /// Replaces the older `execute(op)` shortcut which dispatched straight to
    /// `execute_*` without any preflight. `execute(op)` is retained below as a
    /// deprecated back-compat shim that synthesizes a fresh plan (so at least
    /// the preflight runs) — new code and all UI/headless paths must use `run`.
    pub fn run(
        &mut self,
        op: &Operation,
        plan: &OperationPlan,
    ) -> Result<OperationOutcome, GitError> {
        // ── Preflight: refuse if the repo changed between plan and execute. ──
        match op {
            Operation::StashApply { .. } | Operation::StashPop { .. } => {
                // Stash ops also verify the stash list hasn't shifted.
                self.preflight_check_stash(plan, plan.stash_count_at_plan())?;
            }
            // Discard/DeleteBranch already re-plan in the legacy execute path;
            // keep a HEAD preflight for the rest. Commit is non-mutating of
            // HEAD in a way the preflight detects (it advances HEAD), so it is
            // also gated: a confirmed commit plan captures the pre-commit HEAD.
            _ => self.preflight_check(plan)?,
        }

        // ── Dispatch (behaviour-identical to the former execute(op)). ──
        match op {
            Operation::Commit { message } => {
                self.execute_commit(message).map(OperationOutcome::Commit)
            }
            Operation::MergeCommit { message } => self
                .execute_merge_commit(message)
                .map(OperationOutcome::Commit),
            Operation::Checkout { branch } => self
                .execute_checkout(branch)
                .map(|()| OperationOutcome::Unit),
            Operation::CheckoutCommit { id } => self
                .execute_checkout_commit(id)
                .map(|()| OperationOutcome::Unit),
            Operation::CreateBranch { name, at } => self
                .execute_create_branch(name, at)
                .map(|()| OperationOutcome::Unit),
            Operation::CreateBranchWithCheckout {
                name,
                at,
                checkout_after,
            } => {
                self.execute_create_branch(name, at)?;
                if *checkout_after {
                    self.execute_checkout(name)?;
                }
                Ok(OperationOutcome::Unit)
            }
            Operation::CreateWorktree {
                branch,
                path,
                start,
            } => self
                .execute_create_worktree(branch, path.as_str(), start)
                .map(|()| OperationOutcome::Unit),
            Operation::OpenWorktreeForBranch { branch, path } => self
                .execute_open_worktree_for_branch(branch, path.as_str())
                .map(|()| OperationOutcome::Unit),
            Operation::StashPush {
                message,
                include_untracked,
            } => self
                .execute_stash_push(message.as_deref(), *include_untracked)
                .map(|()| OperationOutcome::Unit),
            Operation::StashApply { index } => self
                .execute_stash_apply(*index)
                .map(|()| OperationOutcome::Unit),
            Operation::StashPop { index } => self
                .execute_stash_pop(*index)
                .map(|()| OperationOutcome::Unit),
            Operation::CherryPick { id } => {
                self.execute_cherry_pick(id).map(OperationOutcome::Commit)
            }
            Operation::MergeBranch { target } => self
                .execute_merge_branch(target)
                .map(OperationOutcome::Commit),
            Operation::MergeIntoConflict { target } => self
                .execute_merge_into_conflict(target)
                .map(OperationOutcome::MergeIntoConflict),
            Operation::CheckoutTrackingBranch {
                remote_branch,
                local_branch,
            } => self
                .execute_checkout_tracking_branch(remote_branch, local_branch)
                .map(|()| OperationOutcome::Unit),
            Operation::SwitchToLatestBranch {
                branch_name,
                remote_branch,
            } => self
                .execute_switch_to_latest(plan, branch_name, remote_branch)
                .map(|()| OperationOutcome::Unit),
            Operation::Revert { id } => self.execute_revert(id).map(OperationOutcome::Commit),
            Operation::Pull => self.execute_pull().map(OperationOutcome::Pull),
            Operation::Push => self.execute_push().map(OperationOutcome::Push),
            Operation::PullBranchFf { branch_name } => self
                .execute_pull_branch_ff(plan, branch_name)
                .map(OperationOutcome::Pull),
            Operation::PushBranch {
                branch_name,
                set_upstream,
            } => self
                .execute_push_branch(plan, branch_name, *set_upstream)
                .map(OperationOutcome::Push),
            Operation::SetUpstream {
                branch_name,
                upstream,
            } => self
                .execute_set_upstream(plan, branch_name, upstream)
                .map(|()| OperationOutcome::Unit),
            Operation::RenameBranch { old_name, new_name } => self
                .execute_rename_branch(plan, old_name, new_name)
                .map(|()| OperationOutcome::Unit),
            Operation::UndoCommit => self.execute_undo_commit().map(OperationOutcome::Undo),
            Operation::Amend { mode, message } => self
                .execute_amend(*mode, message.as_deref())
                .map(OperationOutcome::Amend),
            Operation::DeleteBranch { name } => self
                .execute_delete_branch(plan, name)
                .map(|()| OperationOutcome::Unit),
            Operation::Discard { paths } => self
                .execute_discard(plan, paths)
                .map(OperationOutcome::Discard),
        }
    }

    pub fn plan_commit(&self, message: &str) -> Result<OperationPlan, GitError> {
        staging::plan_commit(&self.repo, message)
    }

    pub fn execute_commit(&self, message: &str) -> Result<CommitId, GitError> {
        staging::execute_commit(&self.repo, message)
    }

    /// Plan a merge-finalize commit (`git commit` with `MERGE_HEAD` present).
    /// The plan captures the current HEAD so `run()`'s preflight can detect a
    /// change between plan and execute (ADR-0104). The conflict-resolution save
    /// IS the substantive work; this plan is the safety gate around it.
    pub fn plan_merge_commit(&self, message: &str) -> Result<OperationPlan, GitError> {
        // Reuse plan_commit's HEAD snapshot + status capture; the merge-commit
        // message is informational (not validated against the staged tree).
        let mut plan = self.plan_commit(message)?;
        plan.title = "Finalize merge commit".to_string();
        Ok(plan)
    }

    pub fn detect_conflict_session(&self) -> Option<conflicts::ConflictSession> {
        conflicts::detect_conflict_session(&self.repo)
    }

    pub fn resolution_buffer_from_repo(&self) -> Result<ResolutionBuffer, GitError> {
        ResolutionBuffer::from_repo(&self.repo)
    }

    pub fn materialized_markers(&self, buffer: &ResolutionBuffer, path: &Path) -> Option<String> {
        buffer.materialized_markers(&self.repo, path)
    }

    pub fn continue_blockers(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Vec<conflicts::ContinueBlocker> {
        conflicts::continue_blockers(&self.repo, session, buffer)
    }

    pub fn plan_conflict_continue(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_continue(&self.repo, session, buffer)
    }

    pub fn plan_conflict_continue_route(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
        current_branch: &str,
    ) -> Result<conflicts::ContinueRoute, GitError> {
        conflicts::plan_conflict_continue_route(&self.repo, session, buffer, current_branch)
    }

    pub fn execute_conflict_continue(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::ContinueOutcome, GitError> {
        conflicts::execute_conflict_continue(&self.repo, session, buffer)
    }

    pub fn execute_conflict_save(
        &self,
        buffer: &ResolutionBuffer,
        path: &Path,
    ) -> Result<conflicts::SaveOutcome, GitError> {
        conflicts::execute_conflict_save(&self.repo, buffer, path)
    }

    /// Materialize + stage every resolved buffer file (collapsing unmerged index
    /// stages → stage 0) without creating a commit. Used by the UI merge route
    /// before opening the commit panel, so the index carries no unmerged entries
    /// and the staged resolutions are visible to the Commit button.
    pub fn stage_conflict_resolution(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<(), GitError> {
        conflicts::stage_conflict_resolution(&self.repo, session, buffer)
    }

    pub fn execute_merge_commit(&self, message: &str) -> Result<CommitId, GitError> {
        conflicts::execute_merge_commit(&self.repo, message)
    }

    pub fn plan_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_abort(&self.repo, session)
    }

    pub fn execute_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::AbortOutcome, GitError> {
        conflicts::execute_conflict_abort(&self.repo, session, buffer)
    }

    pub fn plan_conflict_skip(
        &self,
        session: &conflicts::ConflictSession,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_skip(&self.repo, session)
    }

    pub fn execute_conflict_skip(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::SkipOutcome, GitError> {
        conflicts::execute_conflict_skip(&self.repo, session, buffer)
    }

    pub fn create_branch_name_errors(&self, name: &str) -> Vec<ops::BranchNameError> {
        ops::create_branch_name_errors(&self.repo, name)
    }

    pub fn validate_worktree_path_keyed(
        &self,
        input: impl AsRef<Path>,
    ) -> Result<PathBuf, ops::WorktreeValidationError> {
        let repo_root = self.repo.workdir().unwrap_or(self.path.as_path());
        ops::validate_worktree_path_keyed(repo_root, input)
    }

    pub fn plan_checkout(&self, branch: &str) -> Result<OperationPlan, GitError> {
        ops::plan_checkout(&self.repo, branch)
    }

    pub fn preflight_check(&self, plan: &OperationPlan) -> Result<(), GitError> {
        ops::preflight_check(&self.repo, plan)
    }

    pub fn execute_checkout(&self, branch: &str) -> Result<(), GitError> {
        ops::execute_checkout(&self.repo, branch)
    }

    pub fn plan_checkout_commit(&self, id: &CommitId) -> Result<OperationPlan, GitError> {
        ops::plan_checkout_commit(&self.repo, id)
    }

    pub fn execute_checkout_commit(&self, id: &CommitId) -> Result<(), GitError> {
        ops::execute_checkout_commit(&self.repo, id)
    }

    pub fn plan_create_branch(&self, name: &str, at: &CommitId) -> Result<OperationPlan, GitError> {
        ops::plan_create_branch(&self.repo, name, at)
    }

    pub fn execute_create_branch(&self, name: &str, at: &CommitId) -> Result<(), GitError> {
        ops::execute_create_branch(&self.repo, name, at)
    }

    pub fn plan_create_branch_with_checkout(
        &self,
        name: &str,
        at: &CommitId,
        checkout_after: bool,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_create_branch_with_checkout(&self.repo, name, at, checkout_after)
    }

    pub fn plan_create_worktree(
        &self,
        branch: &str,
        path: impl AsRef<Path>,
        start: &CommitId,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_create_worktree(&self.repo, branch, path, start)
    }

    pub fn plan_open_worktree_for_branch(
        &self,
        branch: &str,
        path: impl AsRef<Path>,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_open_worktree_for_branch(&self.repo, branch, path)
    }

    pub fn execute_create_worktree(
        &self,
        branch: &str,
        path: impl AsRef<Path>,
        start: &CommitId,
    ) -> Result<(), GitError> {
        ops::execute_create_worktree(&self.repo, branch, path, start)
    }

    pub fn execute_open_worktree_for_branch(
        &self,
        branch: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), GitError> {
        ops::execute_open_worktree_for_branch(&self.repo, branch, path)
    }

    pub fn plan_stash_push(
        &mut self,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_stash_push(&mut self.repo, message, include_untracked)
    }

    pub fn execute_stash_push(
        &mut self,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<(), GitError> {
        ops::execute_stash_push(&mut self.repo, message, include_untracked)
    }

    pub fn plan_stash_apply(&mut self, index: usize) -> Result<OperationPlan, GitError> {
        ops::plan_stash_apply(&mut self.repo, index)
    }

    pub fn execute_stash_apply(&mut self, index: usize) -> Result<(), GitError> {
        ops::execute_stash_apply(&mut self.repo, index)
    }

    pub fn plan_stash_pop(&mut self, index: usize) -> Result<OperationPlan, GitError> {
        ops::plan_stash_pop(&mut self.repo, index)
    }

    pub fn execute_stash_pop(&mut self, index: usize) -> Result<(), GitError> {
        ops::execute_stash_pop(&mut self.repo, index)
    }

    pub fn plan_stash_drop(&mut self, index: usize) -> Result<OperationPlan, GitError> {
        ops::plan_stash_drop(&mut self.repo, index)
    }

    pub fn execute_stash_drop(&mut self, index: usize) -> Result<String, GitError> {
        ops::execute_stash_drop(&mut self.repo, index)
    }

    pub fn preflight_check_stash(
        &mut self,
        plan: &OperationPlan,
        expected_stash_count: usize,
    ) -> Result<(), GitError> {
        ops::preflight_check_stash(&mut self.repo, plan, expected_stash_count)
    }

    pub fn plan_cherry_pick(&self, id: &CommitId) -> Result<OperationPlan, GitError> {
        ops::plan_cherry_pick(&self.repo, id)
    }

    pub fn execute_cherry_pick(&self, id: &CommitId) -> Result<CommitId, GitError> {
        ops::execute_cherry_pick(&self.repo, id)
    }

    pub fn plan_merge_branch(&self, target: &str) -> Result<(OperationPlan, MergeKind), GitError> {
        ops::plan_merge_branch(&self.repo, target)
    }

    pub fn execute_merge_branch(&self, target: &str) -> Result<CommitId, GitError> {
        ops::execute_merge_branch(&self.repo, target)
    }

    pub fn execute_merge_into_conflict(&self, target: &str) -> Result<Vec<String>, GitError> {
        ops::execute_merge_into_conflict(&self.repo, target)
    }

    pub fn plan_checkout_tracking_branch(
        &self,
        remote_branch: &str,
        local_branch: &str,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_checkout_tracking_branch(&self.repo, remote_branch, local_branch)
    }

    pub fn execute_checkout_tracking_branch(
        &self,
        remote_branch: &str,
        local_branch: &str,
    ) -> Result<(), GitError> {
        ops::execute_checkout_tracking_branch(&self.repo, remote_branch, local_branch)
    }

    pub fn plan_switch_to_latest(
        &self,
        branch_name: &str,
        remote_branch: &str,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_switch_to_latest(&self.repo, branch_name, remote_branch)
    }

    pub fn execute_switch_to_latest(
        &self,
        plan: &OperationPlan,
        branch_name: &str,
        remote_branch: &str,
    ) -> Result<(), GitError> {
        ops::execute_switch_to_latest(&self.repo, &self.path, plan, branch_name, remote_branch)
    }

    pub fn plan_revert(&self, id: &CommitId) -> Result<OperationPlan, GitError> {
        ops::plan_revert(&self.repo, id)
    }

    pub fn execute_revert(&self, id: &CommitId) -> Result<CommitId, GitError> {
        ops::execute_revert(&self.repo, id)
    }

    pub fn plan_pull(&self) -> Result<OperationPlan, GitError> {
        ops::plan_pull(&self.repo)
    }

    pub fn execute_pull(&self) -> Result<PullOutcome, GitError> {
        ops::execute_pull(&self.repo, &self.path)
    }

    pub fn fetch_remote(&self) -> Result<FetchOutcome, GitError> {
        ops::fetch_remote(&self.repo, &self.path)
    }

    pub fn plan_push(&self) -> Result<OperationPlan, GitError> {
        ops::plan_push(&self.repo)
    }

    pub fn execute_push(&self) -> Result<PushOutcome, GitError> {
        ops::execute_push(&self.repo, &self.path)
    }

    pub fn plan_pull_branch_ff(&self, branch_name: &str) -> Result<OperationPlan, GitError> {
        ops::plan_pull_branch_ff(&self.repo, branch_name)
    }

    pub fn execute_pull_branch_ff(
        &self,
        plan: &OperationPlan,
        branch_name: &str,
    ) -> Result<PullOutcome, GitError> {
        ops::execute_pull_branch_ff(&self.repo, &self.path, plan, branch_name)
    }

    pub fn plan_push_branch(
        &self,
        branch_name: &str,
        set_upstream: bool,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_push_branch(&self.repo, branch_name, set_upstream)
    }

    pub fn execute_push_branch(
        &self,
        plan: &OperationPlan,
        branch_name: &str,
        set_upstream: bool,
    ) -> Result<PushOutcome, GitError> {
        ops::execute_push_branch(&self.repo, &self.path, plan, branch_name, set_upstream)
    }

    pub fn plan_set_upstream(
        &self,
        branch_name: &str,
        upstream: &str,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_set_upstream(&self.repo, branch_name, upstream)
    }

    pub fn execute_set_upstream(
        &self,
        plan: &OperationPlan,
        branch_name: &str,
        upstream: &str,
    ) -> Result<(), GitError> {
        ops::execute_set_upstream(&self.repo, plan, branch_name, upstream)
    }

    pub fn plan_rename_branch(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_rename_branch(&self.repo, old_name, new_name)
    }

    pub fn execute_rename_branch(
        &self,
        plan: &OperationPlan,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), GitError> {
        ops::execute_rename_branch(&self.repo, plan, old_name, new_name)
    }

    pub fn validate_branch_rename(
        &self,
        old_name: &str,
        new_name: &str,
        existing_branches: &[String],
    ) -> BranchRenameValidation {
        let _ = self;
        ops::validate_branch_rename(old_name, new_name, existing_branches)
    }

    pub fn plan_undo_commit(&self) -> Result<OperationPlan, GitError> {
        ops::plan_undo_commit(&self.repo)
    }

    pub fn execute_undo_commit(&self) -> Result<UndoOutcome, GitError> {
        ops::execute_undo_commit(&self.repo)
    }

    // ── Operation Undo / Redo (T-UNDOREDO-001, ADR-0081) ──────

    /// Plan an undo of a recorded ref-moving operation (move `branch` from
    /// `after` back to `before`).
    pub fn plan_undo(&self, entry: &HistoryEntry) -> Result<OperationPlan, GitError> {
        ops::plan_undo(
            &self.repo,
            entry.kind.slug(),
            &entry.branch,
            &entry.before,
            &entry.after,
        )
    }

    /// Plan a redo of a recorded ref-moving operation (move `branch` from
    /// `before` forward to `after`).
    pub fn plan_redo(&self, entry: &HistoryEntry) -> Result<OperationPlan, GitError> {
        ops::plan_redo(
            &self.repo,
            entry.kind.slug(),
            &entry.branch,
            &entry.before,
            &entry.after,
        )
    }

    /// Execute an undo: safe ref move of `branch` from `after` to `before`.
    pub fn execute_undo(&self, entry: &HistoryEntry) -> Result<ops::HistoryMoveOutcome, GitError> {
        ops::execute_undo(&self.repo, &entry.branch, &entry.before, &entry.after)
    }

    /// Execute a redo: safe ref move of `branch` from `before` to `after`.
    pub fn execute_redo(&self, entry: &HistoryEntry) -> Result<ops::HistoryMoveOutcome, GitError> {
        ops::execute_redo(&self.repo, &entry.branch, &entry.before, &entry.after)
    }

    /// Read the current branch's reflog into an undo/redo history seed
    /// (ADR-0084). Entries are ordered oldest → newest, ready for
    /// [`kagi_domain::history::OperationHistory::seeded`], so undo/redo works on
    /// a freshly-opened repository (no in-session operations required).
    pub fn history_from_reflog(&self) -> Result<Vec<HistoryEntry>, GitError> {
        ops::history_from_reflog(&self.repo)
    }

    pub fn plan_amend(
        &self,
        mode: AmendMode,
        message: Option<&str>,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_amend(&self.repo, mode, message)
    }

    pub fn execute_amend(
        &self,
        mode: AmendMode,
        message: Option<&str>,
    ) -> Result<AmendOutcome, GitError> {
        ops::execute_amend(&self.repo, mode, message)
    }

    pub fn plan_delete_branch(&self, name: &str) -> Result<OperationPlan, GitError> {
        ops::plan_delete_branch(&self.repo, name)
    }

    pub fn execute_delete_branch(&self, plan: &OperationPlan, name: &str) -> Result<(), GitError> {
        ops::execute_delete_branch(&self.repo, plan, name)
    }

    pub fn plan_discard(&self, paths: &[String]) -> Result<OperationPlan, GitError> {
        ops::plan_discard(&self.repo, paths)
    }

    pub fn execute_discard(
        &self,
        plan: &OperationPlan,
        paths: &[String],
    ) -> Result<DiscardOutcome, GitError> {
        ops::execute_discard(&self.repo, plan, paths)
    }
}
