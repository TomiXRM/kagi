//! Terminal auto-lock surface of the [`Backend`] (#772 Phase 1, ADR-0208):
//! which linked worktree this repository is, its identity, and the
//! token/identity-checked release. Acquisition is the ordinary
//! `plan_lock_worktree` with a token reason.

use super::Backend;
use crate::{ops, GitError, OperationPlan};
use kagi_domain::remove::WorktreeId;
use kagi_domain::worktree_autolock::AutoUnlockTarget;

impl Backend {
    /// The registry name of this repository when it is a **linked** worktree
    /// (`<common>/worktrees/<name>`), `None` for the main worktree. The name
    /// the worktree lock ops take (#772).
    pub fn linked_worktree_name(&self) -> Option<String> {
        if !self.repo.is_worktree() {
            return None;
        }
        self.repo
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// #772: the identity a terminal auto-lock records for the linked worktree
    /// registered as `name` (see `linked_worktree_identity`).
    pub fn linked_worktree_identity(&self, name: &str) -> Result<WorktreeId, GitError> {
        ops::linked_worktree_identity(&self.repo, name)
    }

    pub fn plan_auto_unlock_worktree(
        &self,
        name: &str,
        target: &AutoUnlockTarget,
    ) -> Result<OperationPlan, GitError> {
        ops::plan_auto_unlock_worktree(&self.repo, name, target)
    }

    pub fn execute_auto_unlock_worktree(
        &self,
        plan: &OperationPlan,
        name: &str,
        target: &AutoUnlockTarget,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        ops::execute_auto_unlock_worktree(&self.repo, plan, name, target)
    }
}
