//! CherryRevertNote — ADR-0129 Phase 2 category (structured by this op's fan-out PR).
//!
//! Empty until the corresponding `ops/cherry_revert.rs` PR converts its Verbatim
//! notes; keeping the enum (and its dispatch arms) pre-wired means the
//! parallel per-op PRs never touch the shared enum/dispatch files.

/// Plan notes for the cherry_revert op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertNote {}

impl CherryRevertNote {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}

/// Plan titles for the cherry_revert op family (filled by its fan-out PR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertTitle {}

impl CherryRevertTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}

/// Recovery kinds for the cherry_revert op family (filled by its fan-out PR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertRecovery {}

impl CherryRevertRecovery {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}
