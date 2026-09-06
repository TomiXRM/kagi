//! Conflict execution stays behind owner trust, including frozen D/F plans.
use super::*;

impl Backend {
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
    ) -> Result<conflicts::ContinueResult, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_continue(&self.repo, &self.path, session, buffer)
    }

    pub fn execute_conflict_save(
        &self,
        buffer: &ResolutionBuffer,
        path: &Path,
    ) -> Result<conflicts::SaveOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_save(&self.repo, buffer, path)
    }

    /// Resolve a directory/file conflict (#320) by keeping one side wholesale,
    /// staging the result into the index and recording it to the oplog. Mirrors
    /// [`Self::execute_conflict_save`]: no commit, the caller re-detects so the
    /// resolved path leaves the conflict set.
    pub fn execute_dir_file_resolution(
        &self,
        path: &Path,
        choice: crate::ops::DirFileChoice,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        let plan = crate::ops::plan_dir_file_resolution(&self.repo, path, choice)?;
        self.execute_planned_dir_file_resolution(&plan)
    }

    /// Execute the reviewed D/F plan without replacing its frozen index facts.
    pub fn execute_planned_dir_file_resolution(
        &self,
        plan: &ops::DirFilePlan,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        ops::execute_dir_file_resolution(&self.repo, &self.path, plan)
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
        self.require_trust()?;
        conflicts::stage_conflict_resolution(&self.repo, session, buffer)
    }

    pub(crate) fn execute_merge_commit(&self, message: &str) -> Result<CommitId, GitError> {
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
        self.require_trust()?;
        conflicts::execute_conflict_abort(&self.repo, session, buffer)
    }

    /// #309: abort a stash-conflict — restore HEAD for the conflicted paths and
    /// clear their unmerged index entries, leaving the stash entry intact. This
    /// is NOT [`Self::execute_conflict_abort`] (which moves refs via ORIG_HEAD);
    /// a conflicted stash apply writes no ORIG_HEAD / sequencer state.
    pub fn execute_stash_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::AbortOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_stash_conflict_abort(&self.repo, session, buffer)
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
        self.require_trust()?;
        conflicts::execute_conflict_skip(&self.repo, session, buffer)
    }
}
