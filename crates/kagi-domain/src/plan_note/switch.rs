//! SwitchNote — ADR-0129 appendix §B-2 (switch op family).
//!
//! Covers `crates/kagi-git/src/ops/switch.rs`'s two plan functions:
//! `plan_checkout_tracking_branch` (create a local branch tracking a remote
//! ref, then check it out — T-BCM-061) and `plan_switch_to_latest` (fetch +
//! switch, fast-forwarding only when safe — ADR-0101). Cross-op templates
//! (conflicted / dirty / untracked / error-passthrough) live in
//! [`super::common::CommonNote`] (§A) — this module only carries the
//! switch-specific templates.

/// Plan notes for the switch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchNote {
    /// blocker — tracking-checkout: the new local branch name is empty.
    LocalNameEmpty,
    /// blocker — tracking-checkout: a local branch with that name already exists.
    LocalExists { name: String },
    /// blocker — switch-to-latest: the branch name is empty.
    NameEmpty,
    /// blocker — switch-to-latest: no upstream/remote branch was given.
    NoUpstreamToSwitch,
    /// warning — switch-to-latest: the local branch doesn't exist yet and will
    /// be created tracking `remote`.
    WillCreateTracking { name: String, remote: String },
    /// warning — switch-to-latest: local knowledge says a clean fast-forward
    /// of `behind` commit(s) is available (re-checked after fetch).
    FfLocalKnowledge { behind: usize },
    /// warning — switch-to-latest: the local branch is ahead of `remote`;
    /// switching only, not updated.
    AheadSwitchOnly {
        name: String,
        ahead: usize,
        remote: String,
    },
    /// warning — switch-to-latest: the local branch has diverged from `remote`.
    DivergedSwitchOnly {
        name: String,
        remote: String,
        ahead: usize,
        behind: usize,
    },
}

impl SwitchNote {
    /// Byte-identical to the legacy `ops/switch.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            SwitchNote::LocalNameEmpty => "Local branch name is empty.".to_string(),
            SwitchNote::LocalExists { name } => {
                format!("Local branch '{}' already exists.", name)
            }
            SwitchNote::NameEmpty => "Branch name is empty.".to_string(),
            SwitchNote::NoUpstreamToSwitch => "No upstream/remote branch to switch to.".to_string(),
            SwitchNote::WillCreateTracking { name, remote } => format!(
                "Local branch '{}' does not exist; it will be created tracking {}.",
                name, remote
            ),
            SwitchNote::FfLocalKnowledge { behind } => format!(
                "Fast-forward {} commit(s) (local knowledge; re-checked after fetch).",
                behind
            ),
            SwitchNote::AheadSwitchOnly {
                name,
                ahead,
                remote,
            } => format!(
                "'{}' is {} commit(s) ahead of {}; switching only, not updated.",
                name, ahead, remote
            ),
            SwitchNote::DivergedSwitchOnly {
                name,
                remote,
                ahead,
                behind,
            } => format!(
                "'{}' has diverged from {} ({} ahead, {} behind); switching only — \
                 merge or rebase to integrate.",
                name, remote, ahead, behind
            ),
        }
    }
}

/// Plan titles for the switch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchTitle {
    /// `Checkout {remote} as local branch {local}`.
    CheckoutTracking { remote: String, local: String },
    /// `Switch to latest {branch} (fetch {remote})`.
    SwitchToLatest { branch: String, remote: String },
}

impl SwitchTitle {
    /// Byte-identical to the legacy `ops/switch.rs` title strings.
    pub fn message_en(&self) -> String {
        match self {
            SwitchTitle::CheckoutTracking { remote, local } => {
                format!("Checkout {} as local branch {}", remote, local)
            }
            SwitchTitle::SwitchToLatest { branch, remote } => {
                format!("Switch to latest {} (fetch {})", branch, remote)
            }
        }
    }
}

/// Recovery kinds for the switch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchRecovery {
    /// tracking-checkout: switch back and delete the new branch if unwanted.
    CheckoutTracking { local: String },
    /// switch-to-latest: what fetch+switch will and won't move.
    SwitchToLatest { remote: String, branch: String },
}

impl SwitchRecovery {
    /// Byte-identical to the legacy `ops/switch.rs` recovery strings.
    pub fn message_en(&self) -> String {
        match self {
            SwitchRecovery::CheckoutTracking { local } => format!(
                "If checkout succeeds but you do not want the branch, switch back and delete it:\n  git checkout -\n  git branch -d {}",
                local
            ),
            SwitchRecovery::SwitchToLatest { remote, branch } => format!(
                "Fetches {} then switches to {}, fast-forwarding only when safe. \
                 Diverged/ahead branches are switched to but never moved. \
                 To go back: git checkout -",
                remote, branch
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // `ops/switch.rs` producer strings (appendix §B-2 / §C / §D).

    #[test]
    fn local_name_empty() {
        assert_eq!(
            SwitchNote::LocalNameEmpty.message_en(),
            "Local branch name is empty."
        );
    }

    #[test]
    fn local_exists() {
        assert_eq!(
            SwitchNote::LocalExists {
                name: "feat/x".into()
            }
            .message_en(),
            "Local branch 'feat/x' already exists."
        );
    }

    #[test]
    fn name_empty() {
        assert_eq!(SwitchNote::NameEmpty.message_en(), "Branch name is empty.");
    }

    #[test]
    fn no_upstream_to_switch() {
        assert_eq!(
            SwitchNote::NoUpstreamToSwitch.message_en(),
            "No upstream/remote branch to switch to."
        );
    }

    #[test]
    fn will_create_tracking() {
        assert_eq!(
            SwitchNote::WillCreateTracking {
                name: "feat/x".into(),
                remote: "origin/feat/x".into()
            }
            .message_en(),
            "Local branch 'feat/x' does not exist; it will be created tracking origin/feat/x."
        );
    }

    #[test]
    fn ff_local_knowledge() {
        assert_eq!(
            SwitchNote::FfLocalKnowledge { behind: 3 }.message_en(),
            "Fast-forward 3 commit(s) (local knowledge; re-checked after fetch)."
        );
    }

    #[test]
    fn ahead_switch_only() {
        assert_eq!(
            SwitchNote::AheadSwitchOnly {
                name: "feat/x".into(),
                ahead: 2,
                remote: "origin/feat/x".into()
            }
            .message_en(),
            "'feat/x' is 2 commit(s) ahead of origin/feat/x; switching only, not updated."
        );
    }

    #[test]
    fn diverged_switch_only() {
        assert_eq!(
            SwitchNote::DivergedSwitchOnly {
                name: "feat/x".into(),
                remote: "origin/feat/x".into(),
                ahead: 2,
                behind: 5
            }
            .message_en(),
            "'feat/x' has diverged from origin/feat/x (2 ahead, 5 behind); switching only — merge or rebase to integrate."
        );
    }

    #[test]
    fn title_checkout_tracking() {
        assert_eq!(
            SwitchTitle::CheckoutTracking {
                remote: "origin/feat/x".into(),
                local: "feat/x".into()
            }
            .message_en(),
            "Checkout origin/feat/x as local branch feat/x"
        );
    }

    #[test]
    fn title_switch_to_latest() {
        assert_eq!(
            SwitchTitle::SwitchToLatest {
                branch: "master".into(),
                remote: "origin/master".into()
            }
            .message_en(),
            "Switch to latest master (fetch origin/master)"
        );
    }

    #[test]
    fn recovery_checkout_tracking() {
        assert_eq!(
            SwitchRecovery::CheckoutTracking {
                local: "feat/x".into()
            }
            .message_en(),
            "If checkout succeeds but you do not want the branch, switch back and delete it:\n  git checkout -\n  git branch -d feat/x"
        );
    }

    #[test]
    fn recovery_switch_to_latest() {
        assert_eq!(
            SwitchRecovery::SwitchToLatest {
                remote: "origin".into(),
                branch: "master".into()
            }
            .message_en(),
            "Fetches origin then switches to master, fast-forwarding only when safe. Diverged/ahead branches are switched to but never moved. To go back: git checkout -"
        );
    }
}
