//! Execution settings are values supplied by the adapter, never permissions.
use super::*;

/// Common write policy (#494). Trust, preflight and verification cannot be disabled.
/// GUI hosts resolve `auto_snapshot` from Settings; CLI/MCP deliberately default
/// to enabled and never read the GUI settings file. The repository path remains
/// the target: policy cannot redirect a write to the currently selected tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionPolicy {
    pub actor: crate::oplog::Actor,
    pub auto_snapshot: bool,
}
impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self::human(true)
    }
}
impl ExecutionPolicy {
    pub const fn human(auto_snapshot: bool) -> Self {
        Self {
            actor: crate::oplog::Actor::Human,
            auto_snapshot,
        }
    }
    pub const fn cli() -> Self {
        Self {
            actor: crate::oplog::Actor::Cli,
            auto_snapshot: true,
        }
    }
    pub const fn mcp() -> Self {
        Self {
            actor: crate::oplog::Actor::Mcp,
            auto_snapshot: true,
        }
    }
}
impl Backend {
    pub fn open_with_policy(path: &Path, policy: ExecutionPolicy) -> Result<Self, GitError> {
        let mut backend = Self::open(path)?;
        backend.policy = policy;
        Ok(backend)
    }
    pub fn discover_with_policy(path: &Path, policy: ExecutionPolicy) -> Result<Self, GitError> {
        let mut backend = Self::discover(path)?;
        backend.policy = policy;
        Ok(backend)
    }
    pub fn execution_policy(&self) -> ExecutionPolicy {
        self.policy
    }

    /// Best-effort optional savepoint, shared by run and absorb (ADR-0154).
    /// Required restore/discard recovery is deliberately outside this toggle.
    pub(super) fn auto_savepoint(&self, name: &str) -> Option<String> {
        if !self.policy.auto_snapshot {
            return None;
        }
        match ops::create_snapshot(&self.repo, &format!("auto snapshot before {name}")) {
            Ok(snapshot) => {
                let _ = ops::prune_snapshots(&self.repo, ops::DEFAULT_SNAPSHOT_CAP);
                Some(snapshot.commit)
            }
            Err(error) => {
                eprintln!("kagi: auto-snapshot before {name} failed: {error}");
                None
            }
        }
    }
}
