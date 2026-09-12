//! What a write is about to make true on a remote, and what the remote says now.
//!
//! Split from `stash.rs`: a `git push` whose termination could not be proven is
//! reconciled by comparing the ref it promised against the ref that is there —
//! never against whatever the local repository holds later, which an external
//! Git can move (ADR-0177, #702 re-review).
use super::*;

/// A remote effect named before it happens: which ref on which remote, and
/// what it should say afterwards. See [`Backend::remote_expectation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteExpectation {
    pub remote: String,
    /// Fully qualified, e.g. `refs/heads/main` or `refs/tags/v1`.
    pub refname: String,
    pub expect: RemoteExpect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteExpect {
    /// The ref should carry this OID.
    Oid(String),
    /// The ref should be gone.
    Absent,
}

impl RemoteExpectation {
    /// Does what the remote actually has match what was promised?
    pub fn matches(&self, live: Option<&str>) -> bool {
        match &self.expect {
            RemoteExpect::Oid(oid) => live == Some(oid.as_str()),
            RemoteExpect::Absent => live.is_none(),
        }
    }
    /// What was promised, for the observation string.
    pub fn describe(&self) -> String {
        match &self.expect {
            RemoteExpect::Oid(oid) => oid.clone(),
            RemoteExpect::Absent => "absent".to_string(),
        }
    }
}

impl Backend {
    /// What this operation is about to make true on a remote, read **before**
    /// it runs.
    ///
    /// Frozen at approval and compared against a live `ls-remote` when the
    /// termination could not be proven. Never against whatever the repository
    /// holds later: kagi's lease does not stop an external Git from moving the
    /// local branch back to the remote's old tip, and comparing current values
    /// would then call an unlanded push "confirmed" (#702 re-review).
    ///
    /// `None` for an operation whose remote effect cannot be named — a
    /// push of a branch that is not HEAD, a detached HEAD — which leaves the
    /// reconcile read unresolved rather than guessing.
    pub fn remote_expectation(&self, op: &str, plan: &OperationPlan) -> Option<RemoteExpectation> {
        use kagi_domain::plan_note::{
            force_lease::ForceLeaseRecovery, push::PushTitle, remote_branch::RemoteBranchRecovery,
            tag::TagRecovery, PlanTitle, RecoveryKind,
        };
        let kind = plan.recovery.as_ref().map(|recovery| &recovery.kind);
        // The branch and remote come from the plan's own typed title, and the
        // tip from the ref as it stands now: together, what this push is about
        // to put on the remote. `plan_push` and `plan_push_branch` are the same
        // promise under two operation names (#702 review 4).
        if let PlanTitle::Push(
            PushTitle::Push { branch, remote, .. } | PushTitle::PushBranch { branch, remote, .. },
        ) = &plan.title
        {
            if matches!(op, "push" | "branch-push" | "branch-push-set-upstream") {
                let refname = format!("refs/heads/{branch}");
                let oid = self.repo.revparse_single(&refname).ok()?.id().to_string();
                return Some(RemoteExpectation {
                    remote: remote.clone(),
                    refname,
                    expect: RemoteExpect::Oid(oid),
                });
            }
        }
        match (op, kind) {
            (
                "force-with-lease-push",
                Some(RecoveryKind::ForceLease(ForceLeaseRecovery::ForceLeasePush {
                    branch,
                    remote,
                    new_sha,
                    ..
                })),
            ) => Some(RemoteExpectation {
                remote: remote.clone(),
                refname: format!("refs/heads/{branch}"),
                expect: RemoteExpect::Oid(new_sha.clone()),
            }),
            (
                "delete-remote-branch",
                Some(RecoveryKind::RemoteBranch(RemoteBranchRecovery::DeleteRemoteBranch {
                    remote,
                    branch,
                    ..
                })),
            ) => Some(RemoteExpectation {
                remote: remote.clone(),
                refname: format!("refs/heads/{branch}"),
                expect: RemoteExpect::Absent,
            }),
            ("push-tag", Some(RecoveryKind::Tag(TagRecovery::PushTag { name, remote }))) => {
                let oid = self
                    .repo
                    .revparse_single(&format!("refs/tags/{name}"))
                    .ok()?
                    .id()
                    .to_string();
                Some(RemoteExpectation {
                    remote: remote.clone(),
                    refname: format!("refs/tags/{name}"),
                    expect: RemoteExpect::Oid(oid),
                })
            }
            _ => None,
        }
    }

    /// The OID a remote currently has for `refname`, or `None` when the remote
    /// does not have it. `Err` when the remote could not be read at all —
    /// which is not the same answer and must never pass for one (ADR-0177).
    ///
    /// A local snapshot cannot say whether a push landed; this is the only
    /// thing that can, and it is what the reconcile read for a remote-writing
    /// operation asks (#702 re-review).
    pub fn read_remote_ref(
        path: &Path,
        remote: &str,
        refname: &str,
    ) -> Result<Option<String>, GitError> {
        ops::check_operand("remote", remote)?;
        ops::check_operand("ref", refname)?;
        let out = crate::cli::run_git(path, &["ls-remote", "--", remote, refname])
            .map_err(|e| crate::cli::context("ls-remote failed", e))?;
        if out.status != 0 {
            return Err(GitError::Other(format!(
                "ls-remote failed (exit {}): {}",
                out.status,
                out.stderr.trim()
            )));
        }
        Ok(out
            .stdout
            .lines()
            .find_map(|line| line.split_whitespace().next().map(str::to_string)))
    }
}
