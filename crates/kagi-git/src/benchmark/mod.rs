//! Shared, headless infrastructure for #627 backend measurements.
//!
//! The examples are deliberately thin dispatchers. Experiment packages add a
//! [`ProbeOperation`] implementation in their own module; the common fixture,
//! timing, fingerprint, and JSON envelope code remains here.

mod environment;
mod fingerprint;
mod fixture;
mod probe;

pub use environment::{collect_environment, EnvironmentMeta};
pub use fingerprint::{
    fingerprint_repository, Fingerprint, FingerprintEntry, FingerprintKind, FingerprintRoot,
};
pub use fixture::{
    generate_fixture, manifest_path, materialize_pristine, FixtureManifest, FixtureRequest,
};
pub use probe::{run_probe, ProbeContext, ProbeOperation, ProbeReport, ProbeRequest, Timing};

use std::fmt;

/// Errors emitted by the common P0 harness. They name the failing boundary so
/// downstream packages do not need to stringify unrelated I/O or Git errors.
#[derive(Debug)]
pub struct HarnessError {
    message: String,
}

impl HarnessError {
    /// Public because every downstream probe module (`backend_probe/{b,e,…}.rs`)
    /// is an *example* target, i.e. a separate crate: `pub(crate)` puts the only
    /// way to report a harness failure out of their reach (#627 P3).
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Public for the same reason as [`HarnessError::new`].
    pub fn io(path: &std::path::Path, error: std::io::Error) -> Self {
        Self::new(format!("{}: {error}", path.display()))
    }
}

/// Probes drive real `Backend` operations, so a `GitError` is the ordinary way a
/// measurement fails. Without this the `?` operator is unavailable to every
/// downstream probe module and each one grows its own `map_err` (#627 P3).
impl From<crate::GitError> for HarnessError {
    fn from(error: crate::GitError) -> Self {
        Self::new(error.to_string())
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for HarnessError {}

impl From<git2::Error> for HarnessError {
    fn from(error: git2::Error) -> Self {
        Self::new(error.message())
    }
}

impl From<serde_json::Error> for HarnessError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}
