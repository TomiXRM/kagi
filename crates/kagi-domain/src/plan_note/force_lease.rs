//! ForceLeaseNote — force-with-lease push (branch-menu "Advanced / Dangerous"
//! group, "Force-with-lease push...").
//!
//! The ONE exception to `push.rs`'s "force is never used" policy, and it's
//! not a blind force: `--force-with-lease=<branch>:<expected-remote-sha>`
//! makes the remote reject the push if its ref has moved since our last
//! fetch, so the operation aborts instead of silently clobbering someone
//! else's work — the entire reason `--force-with-lease` exists over `--force`.

/// Plan notes for the force-with-lease-push op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForceLeaseNote {
    /// blocker (`plan_force_with_lease_push`) — no upstream is configured for
    /// the current branch, so there is nothing to lease-check against.
    NoUpstream { branch: String },
    /// blocker (`plan_force_with_lease_push`) — the local branch tip already
    /// matches the remote-tracking ref; nothing to push.
    NothingToPush { branch: String },
    /// warning (unconditional) — this force-pushes and rewrites the remote
    /// branch's history.
    RewritesRemoteHistory { branch: String },
    /// warning (unconditional) — the lease value in effect, so the user can
    /// see exactly what "no one else pushed" is being checked against.
    LeaseValue { remote: String, sha: String },
}

impl ForceLeaseNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ForceLeaseNote::NoUpstream { branch } => format!(
                "Branch '{}' has no upstream configured. Force-with-lease needs a known remote tip to lease-check against.",
                branch
            ),
            ForceLeaseNote::NothingToPush { branch } => format!(
                "Branch '{}' already matches its remote-tracking ref. Nothing to force-push.",
                branch
            ),
            ForceLeaseNote::RewritesRemoteHistory { branch } => format!(
                "This overwrites the remote branch '{}''s history. Anyone who already pulled the old history will need to reconcile (e.g. rebase onto the new tip).",
                branch
            ),
            ForceLeaseNote::LeaseValue { remote, sha } => format!(
                "Protected by lease: the push is rejected if '{}' has moved past {} since your last fetch (i.e. if someone else pushed in the meantime).",
                remote, sha
            ),
        }
    }
}

/// Plan titles for the force-with-lease-push op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForceLeaseTitle {
    /// `plan_force_with_lease_push` — `Force-with-lease push '<branch>' to '<remote>'`.
    ForceLeasePush { branch: String, remote: String },
}

impl ForceLeaseTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ForceLeaseTitle::ForceLeasePush { branch, remote } => {
                format!("Force-with-lease push '{}' to '{}'", branch, remote)
            }
        }
    }
}

/// Recovery kinds for the force-with-lease-push op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForceLeaseRecovery {
    /// `plan_force_with_lease_push` — push the remote's pre-force tip back,
    /// itself lease-protected against the just-completed push.
    ForceLeasePush {
        branch: String,
        remote: String,
        previous_remote_sha: String,
        new_sha: String,
    },
}

impl ForceLeaseRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ForceLeaseRecovery::ForceLeasePush {
                branch,
                remote,
                previous_remote_sha,
                new_sha,
            } => format!(
                "The remote's previous tip was '{previous_remote_sha}'. To restore it (itself lease-protected against this push):\n  git push --force-with-lease={branch}:{new_sha} {remote} {previous_remote_sha}:refs/heads/{branch}\nAnyone who pulled the rewritten history will still need to reconcile locally."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_upstream() {
        assert_eq!(
            ForceLeaseNote::NoUpstream {
                branch: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' has no upstream configured. Force-with-lease needs a known remote tip to lease-check against."
        );
    }

    #[test]
    fn nothing_to_push() {
        assert_eq!(
            ForceLeaseNote::NothingToPush {
                branch: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' already matches its remote-tracking ref. Nothing to force-push."
        );
    }

    #[test]
    fn rewrites_remote_history() {
        assert_eq!(
            ForceLeaseNote::RewritesRemoteHistory {
                branch: "feat/x".into()
            }
            .message_en(),
            "This overwrites the remote branch 'feat/x''s history. Anyone who already pulled the old history will need to reconcile (e.g. rebase onto the new tip)."
        );
    }

    #[test]
    fn lease_value() {
        assert_eq!(
            ForceLeaseNote::LeaseValue {
                remote: "origin".into(),
                sha: "a1b2c3d4".into()
            }
            .message_en(),
            "Protected by lease: the push is rejected if 'origin' has moved past a1b2c3d4 since your last fetch (i.e. if someone else pushed in the meantime)."
        );
    }

    #[test]
    fn force_lease_title() {
        assert_eq!(
            ForceLeaseTitle::ForceLeasePush {
                branch: "feat/x".into(),
                remote: "origin".into()
            }
            .message_en(),
            "Force-with-lease push 'feat/x' to 'origin'"
        );
    }

    #[test]
    fn force_lease_recovery() {
        assert_eq!(
            ForceLeaseRecovery::ForceLeasePush {
                branch: "feat/x".into(),
                remote: "origin".into(),
                previous_remote_sha: "a1b2c3d4".into(),
                new_sha: "e5f6a7b8".into(),
            }
            .message_en(),
            "The remote's previous tip was 'a1b2c3d4'. To restore it (itself lease-protected against this push):\n  git push --force-with-lease=feat/x:e5f6a7b8 origin a1b2c3d4:refs/heads/feat/x\nAnyone who pulled the rewritten history will still need to reconcile locally."
        );
    }
}
