//! What a write is about to make true on a remote, and what the remote says now.
//!
//! Split from `stash.rs`: a `git push` whose termination could not be proven is
//! reconciled by comparing the ref it promised against the ref that is there —
//! never against whatever the local repository holds later, which an external
//! Git can move (ADR-0177, #702 re-review).
use super::*;

/// A remote effect named before it happens, and how it will be checked.
///
/// One variant per *kind of observation*, because not every remote effect is a
/// ref: a pull-request merge is server state that only the transport can
/// re-read. The reconcile read dispatches on the variant, so adding a kind
/// adds an arm and nothing else.
///
/// Operations freeze a **list** of these: a batch remote-branch delete is N
/// refs that must *all* be gone before it is confirmed.
/// See [`Backend::remote_expectation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteExpectation {
    /// One ref on one remote, checked with `ls-remote`.
    Ref {
        remote: String,
        /// Fully qualified, e.g. `refs/heads/main` or `refs/tags/v1`.
        refname: String,
        expect: RemoteExpect,
    },
    /// One pull request, checked by asking GitHub again (`gh pr view`). Not a
    /// ref: the merge commit may be anywhere, and the branch may be gone —
    /// what was promised is the PR's own state (#701).
    PullRequest { number: u64, expect: PrExpect },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrExpect {
    /// The pull request should read as merged on the server.
    Merged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteExpect {
    /// The ref should carry this OID.
    Oid(String),
    /// The ref should be gone.
    Absent,
}

/// `owner/name` from a remote URL — the last two path segments, `.git`
/// stripped. Covers both URL shapes Git accepts
/// (`https://host/o/r.git`, `git@host:o/r.git`).
fn repo_identity(url: &str) -> String {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = url.rsplit(['/', ':']);
    let name = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    format!("{owner}/{name}")
}

impl RemoteExpect {
    /// Does what the remote actually has match what was promised?
    pub fn matches(&self, live: Option<&str>) -> bool {
        match self {
            Self::Oid(oid) => live == Some(oid.as_str()),
            Self::Absent => live.is_none(),
        }
    }
    /// What was promised, for the observation string.
    pub fn describe(&self) -> String {
        match self {
            Self::Oid(oid) => oid.clone(),
            Self::Absent => "absent".to_string(),
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
    /// Empty for an operation whose remote effect cannot be named, which leaves
    /// the reconcile read unresolved rather than guessing. A list, because one
    /// operation can promise several refs (a batch delete).
    pub fn remote_expectation(&self, op: &str, plan: &OperationPlan) -> Vec<RemoteExpectation> {
        use kagi_domain::plan_note::{
            cleanup::CleanupRecovery, github::GithubRecovery, RecoveryKind,
        };
        // The one operation that promises *several* refs: the batch of remote
        // halves branch cleanup will delete, frozen into the plan when it was
        // built (#701). `origin` because that is the only remote its
        // `push --delete` touches (`ops::branch_cleanup`).
        if op == "branch-cleanup" {
            if let Some(RecoveryKind::Cleanup(CleanupRecovery::CleanupDelete { remote_refs })) =
                plan.recovery.as_ref().map(|recovery| &recovery.kind)
            {
                return remote_refs
                    .iter()
                    .map(|refname| RemoteExpectation::Ref {
                        remote: "origin".to_string(),
                        refname: refname.clone(),
                        expect: RemoteExpect::Absent,
                    })
                    .collect();
            }
        }
        // The other multi-entry operation: `gh pr merge --delete-branch` is two
        // promises, and confirming the merge alone would let a re-read call an
        // undeleted branch "done" (#701 final review). `observe_remote_run`
        // requires *every* entry, so "merged but the branch is still there" —
        // and "the branch cannot be read" — stay unresolved.
        if op == "pr-merge" {
            if let Some(RecoveryKind::Github(GithubRecovery::MergePr {
                number,
                base_repo,
                delete_branch,
            })) = plan.recovery.as_ref().map(|recovery| &recovery.kind)
            {
                let mut expectations = vec![RemoteExpectation::PullRequest {
                    number: *number,
                    expect: PrExpect::Merged,
                }];
                if let Some(branch) = delete_branch {
                    // Which remote is the PR's base repository? `origin` is a
                    // convention, not a fact, and asking the wrong remote for
                    // `refs/heads/<head>` gets "absent" for a ref that was
                    // never there — a deletion proved by a ref that never
                    // existed (#701 final review 2). With no remote pointing
                    // at the base repository, nothing here is observable, so
                    // freeze *nothing*: an empty expectation never confirms.
                    let Some(remote) = self.remote_for_repo(base_repo) else {
                        return Vec::new();
                    };
                    expectations.push(RemoteExpectation::Ref {
                        remote,
                        refname: format!("refs/heads/{branch}"),
                        expect: RemoteExpect::Absent,
                    });
                }
                return expectations;
            }
        }
        self.one_remote_expectation(op, plan).into_iter().collect()
    }

    /// The name of the local remote pointing at `owner/name`, or `None`.
    ///
    /// GitHub identifies a repository; Git identifies a remote. Only the URL
    /// joins them, so this compares the last two path segments of each remote
    /// URL (`.git` stripped) with the identity the plan froze. Empty
    /// `owner/name` matches nothing — an unknown base repository must not
    /// silently pick the first remote.
    fn remote_for_repo(&self, owner_name: &str) -> Option<String> {
        if owner_name.is_empty() {
            return None;
        }
        let remotes = self.repo.remotes().ok()?;
        for name in remotes.iter().flatten().flatten() {
            let matched = self
                .repo
                .find_remote(name)
                .ok()
                .and_then(|remote| remote.url().ok().map(repo_identity))
                .is_some_and(|id| id.eq_ignore_ascii_case(owner_name));
            if matched {
                return Some(name.to_string());
            }
        }
        None
    }

    fn one_remote_expectation(&self, op: &str, plan: &OperationPlan) -> Option<RemoteExpectation> {
        use kagi_domain::plan_note::{
            force_lease::ForceLeaseRecovery, push::PushTitle, remote_branch::RemoteBranchRecovery,
            tag::TagRecovery, PlanTitle, RecoveryKind,
        };
        let kind = plan.recovery.as_ref().map(|recovery| &recovery.kind);
        // The branch and remote come from the plan's own typed title, and the
        // tip from the ref as it stands now: together, what this push is about
        // to put on the remote. `plan_push` and `plan_push_branch` are the same
        // promise under two operation names (#702 review 4).
        //
        // A `-u` push also writes `branch.<name>.remote` / `.merge` locally,
        // and the remote read says nothing about that half. Deliberately not
        // part of the confirmation (#702 Codex P2): it is a local, idempotent
        // config write that destroys nothing, and its absence is not silent —
        // `plan_push` reads the upstream every time, so the next Push
        // confirmation shows "no upstream" and offers `-u` again. Holding the
        // scope closed over a setting the user can see and redo would cost
        // more than it protects.
        if let PlanTitle::Push(
            PushTitle::Push { branch, remote, .. } | PushTitle::PushBranch { branch, remote, .. },
        ) = &plan.title
        {
            if matches!(op, "push" | "branch-push" | "branch-push-set-upstream") {
                let refname = format!("refs/heads/{branch}");
                let oid = self.repo.revparse_single(&refname).ok()?.id().to_string();
                return Some(RemoteExpectation::Ref {
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
            ) => Some(RemoteExpectation::Ref {
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
            ) => Some(RemoteExpectation::Ref {
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
                Some(RemoteExpectation::Ref {
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
