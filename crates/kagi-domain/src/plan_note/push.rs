//! PushNote / PushTitle / PushRecovery — ADR-0129 appendix §B-5 (+§A, §C, §D).
//!
//! Covers all three `crates/kagi-git/src/ops/push.rs` producers: `plan_push`,
//! `plan_push_branch`, and `plan_set_upstream`. HEAD-state blockers
//! (detached/unborn) and bare `GitError` passthroughs are cross-op notes and
//! stay on [`crate::plan_note::CommonNote`] — they are not redefined here.
//!
//! Two templates carry a **punctuation twin** (ADR-0129 appendix §G-2,
//! "文言ゆれ"): `plan_push` uses an em dash, `plan_push_branch` uses a
//! semicolon. Both are byte-exact, golden-tested producers of the legacy
//! strings — do not merge them until Phase 3.

/// Which punctuation variant a punctuation-twin template uses (appendix §G-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPunct {
    /// `plan_push`'s form (` — …`).
    EmDash,
    /// `plan_push_branch`'s form (`; …`).
    Semicolon,
}

/// Plan notes for the push op family (`plan_push` / `plan_push_branch` /
/// `plan_set_upstream`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushNote {
    /// warning — non-fast-forward pushes are rejected, never forced.
    /// `plan_push` uses [`PushPunct::EmDash`]; `plan_push_branch` uses
    /// [`PushPunct::Semicolon`].
    NoForceUsed { punct: PushPunct },
    /// blocker (`plan_push`) — no upstream configured and no remote exists
    /// anywhere in the repo.
    NoUpstreamNoRemotes { branch: String },
    /// blocker (`plan_push_branch`) — no upstream configured, with the
    /// underlying error rendered (shares its English wording with pull-ff's
    /// `NoUpstream`, but is kept as its own variant here — that category is
    /// out of scope for this PR).
    NoUpstreamWithErr { branch: String, err: String },
    /// blocker (no-op family, §F) — already up to date, nothing to push.
    /// `plan_push` uses [`PushPunct::EmDash`]; `plan_push_branch` uses
    /// [`PushPunct::Semicolon`].
    AlreadyUpToDate { branch: String, punct: PushPunct },
    /// blocker (`plan_set_upstream`) — the given upstream string is not a
    /// `remote/branch`-shaped name.
    UpstreamFormatInvalid,
    /// warning (`plan_set_upstream`) — the named remote-tracking branch has
    /// no local ref yet; the config can still be set.
    UpstreamNotPresentLocally { upstream: String },
}

impl PushNote {
    /// Byte-identical to the legacy `ops/push.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            PushNote::NoForceUsed { punct } => match punct {
                PushPunct::EmDash => {
                    "Non-fast-forward pushes will fail — force is not used.".to_string()
                }
                PushPunct::Semicolon => {
                    "Non-fast-forward pushes will fail; force is not used.".to_string()
                }
            },
            PushNote::NoUpstreamNoRemotes { branch } => format!(
                "No upstream configured for branch '{}' and no remotes exist. \
                 Add a remote with `git remote add origin <url>`.",
                branch
            ),
            PushNote::NoUpstreamWithErr { branch, err } => {
                format!("No upstream configured for branch '{}': {}.", branch, err)
            }
            PushNote::AlreadyUpToDate { branch, punct } => match punct {
                PushPunct::EmDash => format!(
                    "Branch '{}' is already up to date with its upstream — nothing to push.",
                    branch
                ),
                PushPunct::Semicolon => format!(
                    "Branch '{}' is already up to date with its upstream; nothing to push.",
                    branch
                ),
            },
            PushNote::UpstreamFormatInvalid => {
                "Upstream must be a remote branch name like origin/main.".to_string()
            }
            PushNote::UpstreamNotPresentLocally { upstream } => format!(
                "Remote-tracking branch '{}' is not present locally; config can still be set.",
                upstream
            ),
        }
    }
}

/// Plan titles for the push op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushTitle {
    /// `plan_push`, not blocked: `Push '<branch>' to '<remote>' (set upstream)`
    /// / `Push '<branch>' to '<remote>'`.
    Push {
        branch: String,
        remote: String,
        set_upstream: bool,
    },
    /// `plan_push`, blocked (HEAD detached/unborn, or no upstream and no
    /// remotes): `Push (blocked)`.
    PushBlocked,
    /// `plan_push_branch`: `Push '<branch>' to '<remote>/<branch>' (set
    /// upstream)` / `Push '<branch>' to '<remote>'`.
    PushBranch {
        branch: String,
        remote: String,
        set_upstream: bool,
    },
    /// `plan_set_upstream`: `Set upstream of '<branch>' to '<upstream>'`.
    SetUpstream { branch: String, upstream: String },
}

impl PushTitle {
    /// Byte-identical to the legacy `ops/push.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            PushTitle::Push {
                branch,
                remote,
                set_upstream: true,
            } => format!("Push '{}' to '{}' (set upstream)", branch, remote),
            PushTitle::Push { branch, remote, .. } => format!("Push '{}' to '{}'", branch, remote),
            PushTitle::PushBlocked => "Push (blocked)".to_string(),
            PushTitle::PushBranch {
                branch,
                remote,
                set_upstream: true,
            } => format!(
                "Push '{}' to '{}/{}' (set upstream)",
                branch, remote, branch
            ),
            PushTitle::PushBranch { branch, remote, .. } => {
                format!("Push '{}' to '{}'", branch, remote)
            }
            PushTitle::SetUpstream { branch, upstream } => {
                format!("Set upstream of '{}' to '{}'", branch, upstream)
            }
        }
    }
}

/// Recovery kinds for the push op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRecovery {
    /// `plan_push`, not blocked.
    Push,
    /// `plan_push`, blocked (HEAD detached/unborn — no branch to push).
    PushBlocked,
    /// `plan_push_branch`.
    PushBranch,
    /// `plan_set_upstream`.
    SetUpstream { branch: String },
}

impl PushRecovery {
    /// Byte-identical to the legacy `ops/push.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            PushRecovery::Push => {
                "Push only sends commits to the remote — the local repository is never modified.\n\
                 If the push is rejected (non-fast-forward), pull first and re-plan:\n  \
                 git pull\n  git push\n\
                 The reflog records every HEAD movement:\n  git reflog"
                    .to_string()
            }
            PushRecovery::PushBlocked => {
                "Push requires a branch. Use `git checkout <branch>` to attach HEAD.".to_string()
            }
            PushRecovery::PushBranch => {
                "Push sends commits to the remote and does not modify the working tree. \
                 If the push is rejected, fetch or pull first and re-plan."
                    .to_string()
            }
            PushRecovery::SetUpstream { branch } => format!(
                "This changes only branch.{}.remote and branch.{}.merge in git config. \
                 To undo, set the previous upstream again.",
                branch, branch
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // `ops/push.rs` producer strings (dynamic values, backticks, punctuation
    // twins).

    #[test]
    fn no_force_used_both_puncts() {
        assert_eq!(
            PushNote::NoForceUsed {
                punct: PushPunct::EmDash
            }
            .message_en(),
            "Non-fast-forward pushes will fail — force is not used."
        );
        assert_eq!(
            PushNote::NoForceUsed {
                punct: PushPunct::Semicolon
            }
            .message_en(),
            "Non-fast-forward pushes will fail; force is not used."
        );
    }

    #[test]
    fn no_upstream_no_remotes() {
        assert_eq!(
            PushNote::NoUpstreamNoRemotes {
                branch: "feat/x".into()
            }
            .message_en(),
            "No upstream configured for branch 'feat/x' and no remotes exist. \
             Add a remote with `git remote add origin <url>`."
        );
    }

    #[test]
    fn no_upstream_with_err() {
        assert_eq!(
            PushNote::NoUpstreamWithErr {
                branch: "feat/x".into(),
                err: "git error: no upstream for 'feat/x': no upstream configured".into(),
            }
            .message_en(),
            "No upstream configured for branch 'feat/x': git error: no upstream for 'feat/x': \
             no upstream configured."
        );
    }

    #[test]
    fn already_up_to_date_both_puncts() {
        assert_eq!(
            PushNote::AlreadyUpToDate {
                branch: "main".into(),
                punct: PushPunct::EmDash
            }
            .message_en(),
            "Branch 'main' is already up to date with its upstream — nothing to push."
        );
        assert_eq!(
            PushNote::AlreadyUpToDate {
                branch: "main".into(),
                punct: PushPunct::Semicolon
            }
            .message_en(),
            "Branch 'main' is already up to date with its upstream; nothing to push."
        );
    }

    #[test]
    fn upstream_format_invalid() {
        assert_eq!(
            PushNote::UpstreamFormatInvalid.message_en(),
            "Upstream must be a remote branch name like origin/main."
        );
    }

    #[test]
    fn upstream_not_present_locally() {
        assert_eq!(
            PushNote::UpstreamNotPresentLocally {
                upstream: "origin/feat/x".into()
            }
            .message_en(),
            "Remote-tracking branch 'origin/feat/x' is not present locally; \
             config can still be set."
        );
    }

    #[test]
    fn push_title_variants() {
        assert_eq!(
            PushTitle::Push {
                branch: "main".into(),
                remote: "origin".into(),
                set_upstream: true
            }
            .message_en(),
            "Push 'main' to 'origin' (set upstream)"
        );
        assert_eq!(
            PushTitle::Push {
                branch: "main".into(),
                remote: "origin".into(),
                set_upstream: false
            }
            .message_en(),
            "Push 'main' to 'origin'"
        );
        assert_eq!(PushTitle::PushBlocked.message_en(), "Push (blocked)");
    }

    #[test]
    fn push_branch_title_variants() {
        assert_eq!(
            PushTitle::PushBranch {
                branch: "feat/x".into(),
                remote: "origin".into(),
                set_upstream: true
            }
            .message_en(),
            "Push 'feat/x' to 'origin/feat/x' (set upstream)"
        );
        assert_eq!(
            PushTitle::PushBranch {
                branch: "feat/x".into(),
                remote: "origin".into(),
                set_upstream: false
            }
            .message_en(),
            "Push 'feat/x' to 'origin'"
        );
    }

    #[test]
    fn set_upstream_title() {
        assert_eq!(
            PushTitle::SetUpstream {
                branch: "feat/x".into(),
                upstream: "origin/feat/x".into()
            }
            .message_en(),
            "Set upstream of 'feat/x' to 'origin/feat/x'"
        );
    }

    #[test]
    fn push_recovery_text() {
        assert_eq!(
            PushRecovery::Push.message_en(),
            "Push only sends commits to the remote — the local repository is never modified.\n\
             If the push is rejected (non-fast-forward), pull first and re-plan:\n  \
             git pull\n  git push\n\
             The reflog records every HEAD movement:\n  git reflog"
        );
        assert_eq!(
            PushRecovery::PushBlocked.message_en(),
            "Push requires a branch. Use `git checkout <branch>` to attach HEAD."
        );
        assert_eq!(
            PushRecovery::PushBranch.message_en(),
            "Push sends commits to the remote and does not modify the working tree. \
             If the push is rejected, fetch or pull first and re-plan."
        );
        assert_eq!(
            PushRecovery::SetUpstream {
                branch: "feat/x".into()
            }
            .message_en(),
            "This changes only branch.feat/x.remote and branch.feat/x.merge in git config. \
             To undo, set the previous upstream again."
        );
    }
}
