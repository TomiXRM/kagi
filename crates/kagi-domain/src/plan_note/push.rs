//! PushNote — ADR-0129 Phase 2 category (structured by this op's fan-out PR).
//!
//! Empty until the corresponding `ops/push.rs` PR converts its Verbatim
//! notes; keeping the enum (and its dispatch arms) pre-wired means the
//! parallel per-op PRs never touch the shared enum/dispatch files.

/// Plan notes for the push op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushNote {}

impl PushNote {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}

/// Plan titles for the push op family (filled by its fan-out PR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushTitle {}

impl PushTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}

/// Recovery kinds for the push op family (filled by its fan-out PR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRecovery {}

impl PushRecovery {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match *self {}
    }
}
