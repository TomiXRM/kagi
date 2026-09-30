//! Repository-health read for Analyze (#358, ADR-0205). The fixes are
//! ordinary `Operation`s planned and run through [`Backend::plan`] /
//! [`Backend::run`]; this is only the read that decides what to suggest.
use super::*;
use kagi_domain::repo_health::{assess, HealthFinding};

impl Backend {
    /// What Analyze's Health axis should suggest for this repository.
    /// Read-only: file metadata and config, no git subprocess.
    pub fn repo_health(&self) -> Result<Vec<HealthFinding>, GitError> {
        ops::read_health_facts(&self.repo).map(|facts| assess(&facts))
    }
}
