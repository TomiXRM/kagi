use std::path::{Path, PathBuf};

use git2::Repository;

use super::{
    conflicts, diff, diffstat, message_gen, ops, resolution::ResolutionBuffer, resolve_head,
    snapshot, staging, status, AmendMode, AmendOutcome, BranchRenameValidation, CommitId,
    CommitPreview,
    DiscardOutcome, FetchOutcome, FileDiff, FileDiffStat, FileStatus, GitError, Head, MergeKind,
    OperationPlan, PullOutcome, PushOutcome, RepoSnapshot, UndoOutcome, WorkingTreeStatus,
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

    pub fn repo(&self) -> &Repository {
        &self.repo
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

    pub fn commit_changed_files(&self, id: &CommitId) -> Result<Vec<FileStatus>, GitError> {
        diff::commit_changed_files(&self.repo, id)
    }

    pub fn commit_file_diff(&self, id: &CommitId, path: &Path) -> Result<FileDiff, GitError> {
        diff::commit_file_diff(&self.repo, id, path)
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

    pub fn commit_preview(&self) -> Result<CommitPreview, GitError> {
        staging::commit_preview(&self.repo)
    }

    pub fn plan_commit(&self, message: &str) -> Result<OperationPlan, GitError> {
        staging::plan_commit(&self.repo, message)
    }

    pub fn execute_commit(&self, message: &str) -> Result<CommitId, GitError> {
        staging::execute_commit(&self.repo, message)
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
