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
    /// One pull request in one repository, checked by asking GitHub again
    /// (`gh pr view -R <base_repo>`). Not a ref: the merge commit may be
    /// anywhere, and the branch may be gone — what was promised is the PR's
    /// own state (#701).
    PullRequest {
        /// `<host>/<owner>/<repo>`, frozen at plan time and passed to `gh` as
        /// `-R`. A PR number alone is not an address (#701 final review 4).
        base_repo: String,
        number: u64,
        expect: PrExpect,
    },
    /// One branch in one GitHub repository, read through the API rather than
    /// a local remote.
    ///
    /// A remote *name* is not an address: `remote.<name>.url` and
    /// `url.<base>.insteadOf` can both move after the promise is frozen, and
    /// asking the moved endpoint for `refs/heads/<head>` gets "absent" for a
    /// ref that was never there — the deletion confirmed without ever being
    /// observed (#701 final review 4). The git-native families keep [`Self::Ref`];
    /// GitHub operations address the repository they froze.
    GithubRef {
        /// `<host>/<owner>/<repo>`.
        base_repo: String,
        /// Branch name, without `refs/heads/`.
        branch: String,
        expect: RemoteExpect,
    },
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

/// What a live read of a remote ref established — or why it established
/// nothing.
///
/// [`Self::Observed`] is evidence: the remote answered, and carries the OID it
/// has or `None` for a ref it does not have. An `Err` from the read is the
/// opposite of evidence — the transport failed, and nothing at all is known —
/// which is why the two have never been allowed to collapse into one
/// `Option` (ADR-0177).
///
/// [`Self::Unobservable`] is the third answer, and it is about the *address*
/// rather than the network. A promise freezes a remote **name**; by the time
/// it is reconciled that name can be gone, or point through an `ssh_config`
/// alias the configuration no longer maps. There is then no remote to have
/// said "no such ref", so the question cannot be put at all — and saying so is
/// what lets a write scope be released deliberately instead of held forever
/// (#706).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteRefObservation {
    /// The remote answered: the OID it has for the ref, or `None` for a ref it
    /// does not have.
    Observed(Option<String>),
    /// The remote could not be addressed. English, and specific enough to be
    /// the reason a release is recorded under.
    Unobservable { reason: String },
}

/// `<host>/<owner>/<repo>`, lower-cased, from any GitHub-ish URL: a Git remote
/// URL in either shape (`https://host/o/r.git`, `git@host:o/r.git`,
/// `ssh://git@host/o/r`) or a pull-request page URL, whose extra
/// `…/pull/<n>` tail is simply not read.
///
/// The **host is part of the identity**. `acme/widgets` on github.com and
/// `acme/widgets` on a GitHub Enterprise host are different repositories, and
/// picking the wrong one gets "absent" for a ref that was never there (#701
/// final review 3).
///
/// `None` when no host can be read — a local path, a `file://` URL, or an SSH
/// alias whose real host lives in the user's ssh config. Unknown identity is
/// never a guess.
pub(crate) fn repo_identity(url: &str) -> Option<String> {
    let Some((_, rest)) = url.split_once("://") else {
        return scp_identity(url);
    };
    // Strip `user[:pass]@` — only when the `@` is in the authority.
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let rest = match rest[..authority_end].rfind('@') {
        Some(at) => &rest[at + 1..],
        None => rest,
    };
    let (host, path) = rest.split_once('/')?;
    owner_repo(host, path)
}

/// `git@host:owner/repo.git` — Git's scp-like remote syntax, which has no
/// scheme and separates host from path with a colon.
fn scp_identity(url: &str) -> Option<String> {
    let (authority, path) = url.split_once(':')?;
    if authority.contains('/') {
        return None; // a local path that happens to contain a colon
    }
    owner_repo(authority.rsplit('@').next()?, path)
}

/// The identity string itself, given a host that has already been established
/// — literally, from the URL, or through `ssh_config` by
/// [`remote_identity`](super::remote_identity).
pub(super) fn owner_repo(host: &str, path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    let owner = segments.next()?;
    let repo = segments.next()?.trim_end_matches(".git");
    if host.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{host}/{owner}/{repo}").to_ascii_lowercase())
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

/// Why the approval already knows the local head branch stays (#705 review P2).
///
/// Two inputs, both in hand while the modal is still being built: the
/// delete-branch family's own blockers (checked out here, checked out in a
/// linked worktree, detached at the tip) and whether the branch points at the
/// PR head that is about to be merged. Deciding them here is what keeps the
/// plan and the receipt saying the same thing (ADR-0196).
///
/// A branch that is simply **not here** is not a refusal: its absence is the
/// promise the approval freezes, and the receipt reports it as
/// [`PrMergeLocalOutcome::Absent`](kagi_domain::operation::PrMergeLocalOutcome::Absent).
fn pr_merge_keep_reason(
    local: &OperationPlan,
    tip: Option<&str>,
    pr_head: &str,
) -> Option<kagi_domain::plan_note::PrMergeLocalReason> {
    use kagi_domain::plan_note::{CommonNote, PlanNote, PrMergeLocalReason};
    if let Some(blocker) = local
        .blockers
        .iter()
        .find(|note| !matches!(note, PlanNote::Common(CommonNote::BranchMissing { .. })))
    {
        return Some(PrMergeLocalReason::Plan(Box::new(blocker.clone())));
    }
    let tip = tip?;
    // An unknown PR head is its own plan blocker; it cannot also disagree.
    (!pr_head.is_empty() && tip != pr_head).then(|| PrMergeLocalReason::NotAtPrHead {
        tip: tip.to_string(),
        head: pr_head.to_string(),
    })
}

impl Backend {
    /// Freeze local cleanup before the PR merge confirmation is shown.
    ///
    /// Both refusals the executor would raise are decided **here**, while the
    /// modal can still state them: a branch that is checked out somewhere, and
    /// a branch whose tip is not the PR head being merged. They travel as a
    /// `keep_reason`, so the plan promises only what the receipt will report
    /// (#705 review P2).
    pub fn plan_pr_merge(
        &self,
        pr: &kagi_domain::github::PullRequest,
        method: crate::github::MergeMethod,
        delete_branch: bool,
        head_summary: String,
    ) -> Result<OperationPlan, GitError> {
        let local_branch = if delete_branch {
            let local = self.plan_delete_branch(&pr.head)?;
            let tip = match local.recovery.as_ref().map(|r| &r.kind) {
                Some(kagi_domain::plan_note::RecoveryKind::Branch(
                    kagi_domain::plan_note::BranchRecovery::DeleteBranch { tip, .. },
                )) => tip.clone(),
                _ => return Err(GitError::Other("missing local deletion plan".into())),
            };
            // A missing branch is an observation, not an unreadable ref.
            if self.local_branch_tip(&pr.head)? != tip {
                return Err(GitError::Other(
                    "local branch changed while planning".into(),
                ));
            }
            Some(kagi_domain::plan::PrMergeLocalBranch {
                worktree: self.write_worktree_id()?,
                name: pr.head.clone(),
                keep_reason: pr_merge_keep_reason(&local, tip.as_deref(), &pr.head_sha),
                tip,
                head: local.head_at_plan,
            })
        } else {
            None
        };
        Ok(crate::github::plan_pr_merge(
            pr,
            method,
            delete_branch,
            head_summary,
            local_branch,
        ))
    }

    fn local_branch_tip(&self, name: &str) -> Result<Option<String>, GitError> {
        ops::check_operand("branch", name)?;
        let name = format!("refs/heads/{name}");
        match self.repo.find_reference(&name) {
            Ok(reference) => reference
                .target()
                .map(|oid| Some(oid.to_string()))
                .ok_or_else(|| GitError::Other(format!("{name} is not a direct branch ref"))),
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(GitError::Other(error.to_string())),
        }
    }

    /// Revalidate the frozen input, then use the existing locked delete family.
    ///
    /// Every refusal is a [`PrMergeLocalOutcome`], not an `Err`: the merge is
    /// already done when this runs, and each reason reaches a notice that is
    /// rendered in Japanese as well as English, so none of them may be
    /// flattened into an English string here (#705 review P3). `Err` is kept
    /// for the delete itself failing — that is the one case with no frozen
    /// promise left to describe.
    pub(crate) fn execute_pr_merge_local_branch(
        &self,
        branch: &kagi_domain::plan::PrMergeLocalBranch,
        pr_head: &str,
        backup_refs: &mut Vec<String>,
    ) -> Result<kagi_domain::operation::PrMergeLocalOutcome, GitError> {
        use kagi_domain::operation::PrMergeLocalOutcome;
        use kagi_domain::plan_note::PrMergeLocalReason;
        self.require_trust()?;
        let kept = |reason| PrMergeLocalOutcome::NotDeleted {
            name: branch.name.clone(),
            reason,
        };
        // Identity first: a branch name does not say which checkout it came
        // from, so nothing else may be read until this one matches.
        if self.write_worktree_id()? != branch.worktree {
            return Ok(kept(PrMergeLocalReason::IdentityChanged));
        }
        if self.local_branch_tip(&branch.name)? != branch.tip {
            return Ok(kept(PrMergeLocalReason::Changed));
        }
        let Some(tip) = &branch.tip else {
            return Ok(PrMergeLocalOutcome::Absent {
                name: branch.name.clone(),
            });
        };
        if tip != pr_head {
            return Ok(kept(PrMergeLocalReason::NotAtPrHead {
                tip: tip.clone(),
                head: pr_head.to_string(),
            }));
        }
        let plan = self.plan_delete_branch(&branch.name)?;
        if plan.head_at_plan != branch.head {
            return Ok(kept(PrMergeLocalReason::HeadChanged));
        }
        // The delete family's own typed blocker, carried rather than printed.
        if let Some(blocker) = plan.blockers.first() {
            return Ok(kept(PrMergeLocalReason::Plan(Box::new(blocker.clone()))));
        }
        let expected = match plan.recovery.as_ref().map(|r| &r.kind) {
            Some(kagi_domain::plan_note::RecoveryKind::Branch(
                kagi_domain::plan_note::BranchRecovery::DeleteBranch { tip, .. },
            )) => tip.as_ref(),
            _ => None,
        };
        if expected != Some(tip) {
            return Ok(kept(PrMergeLocalReason::Changed));
        }
        let mut partial = None;
        self.execute_delete_branch(&plan, &branch.name, backup_refs, &mut partial)
            .map_err(|error| match partial {
                Some(after) => GitError::Other(format!("{error}; {}", after.dirty)),
                None => error,
            })?;
        Ok(PrMergeLocalOutcome::Deleted {
            name: branch.name.clone(),
            tip: tip.clone(),
        })
    }

    /// Fetch the immutable local inputs used to open a GitHub PR. The
    /// synthetic PR ref works for both same-repository and fork PRs.
    pub fn fetch_pr_refs(
        &self,
        base_repo: &str,
        number: u64,
        base: &str,
        expected_head: &str,
    ) -> Result<(FetchOutcome, CommitId, CommitId), GitError> {
        self.require_trust()?;
        ops::fetch_pr_refs(
            &self.repo,
            &self.path,
            base_repo,
            number,
            base,
            expected_head,
        )
    }

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
        // A PR merge promises *remote* effects only. The local deletion is
        // kagi's own write, it starts only after GitHub confirms the merge,
        // and an unproven termination therefore always leaves it unstarted —
        // so requiring the local ref to be gone could only lock the
        // repository's write scope behind a deletion nobody performed (#705
        // review P1). A fork has no deletable head in the base repository.
        if op == "pr-merge" {
            if let Some(RecoveryKind::Github(GithubRecovery::MergePr {
                number,
                base_repo,
                delete_branch,
                cross_repository,
                local_branch: _,
            })) = plan.recovery.as_ref().map(|recovery| &recovery.kind)
            {
                // Without the repository's identity there is no address to
                // re-read, and a PR number on its own names a different PR in
                // every repository. Freeze *nothing*: an empty expectation
                // never confirms (#701 final review 4).
                if base_repo.is_empty() {
                    return Vec::new();
                }
                let mut expectations = vec![RemoteExpectation::PullRequest {
                    base_repo: base_repo.clone(),
                    number: *number,
                    expect: PrExpect::Merged,
                }];
                if let Some(branch) = delete_branch.as_ref().filter(|_| !cross_repository) {
                    expectations.push(RemoteExpectation::GithubRef {
                        base_repo: base_repo.clone(),
                        branch: branch.clone(),
                        expect: RemoteExpect::Absent,
                    });
                }
                return expectations;
            }
        }
        self.one_remote_expectation(op, plan).into_iter().collect()
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

    /// The OID `base_repo` (`<host>/<owner>/<repo>`) currently has for
    /// `refs/heads/<branch>`, `None` **only** when GitHub answered that the
    /// repository is readable and that exact ref is not in it, and `Err` for
    /// every other answer.
    ///
    /// The repository is addressed directly, so no remote name, no
    /// `remote.<name>.url` and no `url.<base>.insteadOf` sits between the
    /// frozen promise and what is read (#701 review 4). `workdir` is only
    /// `gh`'s working directory, for its auth and host config.
    ///
    /// GraphQL with **variables**, not the REST refs endpoint, because absence
    /// has to be proof (#701 review 5):
    ///
    /// - The branch never enters a URL path. `feature#x` is a valid branch
    ///   name and `#` is a fragment delimiter, so
    ///   `repos/o/r/git/ref/heads/feature#x` is sent as `…/heads/feature` — a
    ///   404 for a ref nobody asked about, read as the absence of one that is
    ///   still there.
    /// - A REST 404 does not mean "no such ref": an invisible repository, lost
    ///   access, or a token without `Contents: read` answers 404 too. A
    ///   preceding `gh pr view` proves nothing about it — different API,
    ///   different permission. So absence is only ever the *structured*
    ///   answer: no `errors`, a non-null `repository`, and a null `ref`.
    pub fn read_github_ref(
        workdir: &Path,
        base_repo: &str,
        branch: &str,
    ) -> Result<Option<String>, GitError> {
        const QUERY: &str = "query($owner:String!,$name:String!,$ref:String!)\
{repository(owner:$owner,name:$name){ref(qualifiedName:$ref){target{oid}}}}";
        let unreadable =
            |what: &str| GitError::Other(format!("gh api graphql {base_repo} {branch}: {what}"));
        let (host, owner_repo) = base_repo
            .split_once('/')
            .ok_or_else(|| GitError::Other(format!("not a repository identity: {base_repo}")))?;
        let (owner, name) = owner_repo
            .split_once('/')
            .ok_or_else(|| GitError::Other(format!("not a repository identity: {base_repo}")))?;
        ops::check_operand("branch", branch)?;
        let out = crate::cli::gh_command()
            .args([
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                &format!("query={QUERY}"),
                "-f",
                &format!("owner={owner}"),
                "-f",
                &format!("name={name}"),
                "-f",
                &format!("ref=refs/heads/{branch}"),
            ])
            .current_dir(workdir)
            .output()
            .map_err(|e| GitError::Other(format!("gh: {e}")))?;
        // A failed command is not an observation, whatever it left on stdout:
        // a `gh` that prints a perfectly shaped `"ref": null` and exits 1 has
        // not read the repository (#701 review 6).
        if !out.status.success() {
            return Err(unreadable(&format!(
                "gh exited {} ({})",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let body: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| {
            unreadable(&format!(
                "no answer ({})",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        })?;
        if body.get("errors").is_some() {
            return Err(unreadable(&body["errors"].to_string()));
        }
        // A null repository is "not visible to this token", not "empty".
        let repository = body
            .get("data")
            .and_then(|d| d.get("repository"))
            .filter(|r| !r.is_null())
            .ok_or_else(|| unreadable("repository not readable"))?;
        match repository.get("ref") {
            Some(r) if r.is_null() => Ok(None),
            Some(r) => r
                .get("target")
                .and_then(|t| t.get("oid"))
                .and_then(|o| o.as_str())
                .map(|oid| Some(oid.to_string()))
                .ok_or_else(|| unreadable("ref without an oid")),
            None => Err(unreadable("no ref field")),
        }
    }

    /// What a remote says `refname` is now — or why it could not be asked.
    ///
    /// A local snapshot cannot say whether a push landed; this is the only
    /// thing that can, and it is what the reconcile read for a remote-writing
    /// operation asks (#702 re-review). The three answers it can come back
    /// with are kept apart deliberately:
    ///
    /// - [`RemoteRefObservation::Observed`] — the remote answered. The OID it
    ///   has, or `None` for a ref it does not have.
    /// - `Err` — the transport failed. Not evidence, and never an absent ref.
    /// - [`RemoteRefObservation::Unobservable`] — there was no address to ask.
    ///   The frozen remote name is not in this repository any more, or it is
    ///   reached over an SSH target whose host the configuration no longer
    ///   maps. Neither can be confirmed or denied by anything (#706).
    ///
    /// The name is checked against the repository *before* the transport,
    /// because `git ls-remote -- <gone> <ref>` reads a missing remote as a URL
    /// and fails at it, which would arrive here as an ordinary transport
    /// failure and hold the scope forever. SSH configuration is resolved before
    /// transport: once `ls-remote` is attempted, every failure stays an error,
    /// never permission to release an unobserved operation.
    pub fn read_remote_ref(
        path: &Path,
        remote: &str,
        refname: &str,
    ) -> Result<RemoteRefObservation, GitError> {
        ops::check_operand("remote", remote)?;
        ops::check_operand("ref", refname)?;
        let (config, url) = remote_target(path, remote)?;
        let Some(url) = url else {
            return Ok(RemoteRefObservation::Unobservable {
                reason: format!("no matching remote \"{remote}\" in this repository"),
            });
        };
        if let Err(unmappable) = super::remote_identity::resolve_repo_identity(&url, &config) {
            return Ok(RemoteRefObservation::Unobservable {
                reason: unmappable.to_string(),
            });
        }
        let out = crate::cli::run_git(path, &["ls-remote", "--", remote, refname])
            .map_err(|e| crate::cli::context("ls-remote failed", e))?;
        if out.status != 0 {
            return Err(GitError::Other(format!(
                "ls-remote failed (exit {}): {}",
                out.status,
                out.stderr.trim()
            )));
        }
        Ok(RemoteRefObservation::Observed(out.stdout.lines().find_map(
            |line| line.split_whitespace().next().map(str::to_string),
        )))
    }
}

/// This repository's configuration, and the URL it has for `remote` now —
/// `None` when it has no remote by that name any more.
///
/// The repository is opened here rather than taken from a [`Backend`]: the
/// reconcile read is a static call against a path, checking a promise frozen
/// by a different run of the program. A repository that cannot be opened or
/// read is an `Err` — that is kagi's own storage failing, which says nothing
/// about whether the remote could be addressed.
///
/// The configuration travels with the URL because identifying an SSH host
/// depends on it: `core.sshCommand` decides whether plain `ssh` is even the
/// program git reaches this remote with.
fn remote_target(path: &Path, remote: &str) -> Result<(git2::Config, Option<String>), GitError> {
    let repo = Repository::open(path).map_err(|e| GitError::Other(e.message().to_string()))?;
    let config = repo
        .config()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    // Bound, not matched in place: the borrow of `repo` ends with this local,
    // which is dropped before the repository it came from.
    let found = repo.find_remote(remote);
    let url = match found {
        Ok(found) => Some(found.url().unwrap_or_default().to_string()),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => return Err(GitError::Other(error.to_string())),
    };
    Ok((config, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two answers a reconcile read may act on, taken from a repository
    /// that really answers: a remote that is there, and a promise whose remote
    /// is not there at all — which is not "the ref is gone" (#706).
    #[test]
    fn a_remote_that_is_gone_is_unobservable_not_an_absent_ref() {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        let bare = Repository::init_bare(&origin).unwrap();
        let tree_id = bare.treebuilder(None).unwrap().write().unwrap();
        let tree = bare.find_tree(tree_id).unwrap();
        let who = git2::Signature::now("Kagi Test", "kagi@example.com").unwrap();
        let tip = bare
            .commit(Some("refs/heads/main"), &who, &who, "base", &tree, &[])
            .unwrap();

        let work = root.path().join("work");
        let repo = Repository::init(&work).unwrap();
        repo.config()
            .unwrap()
            .set_str("remote.origin.url", &format!("file://{}", origin.display()))
            .unwrap();

        assert_eq!(
            Backend::read_remote_ref(&work, "origin", "refs/heads/main").unwrap(),
            RemoteRefObservation::Observed(Some(tip.to_string()))
        );
        assert_eq!(
            Backend::read_remote_ref(&work, "origin", "refs/heads/never").unwrap(),
            RemoteRefObservation::Observed(None),
            "a remote that answers about a ref it lacks has observed its absence"
        );
        match Backend::read_remote_ref(&work, "upstream", "refs/heads/main").unwrap() {
            RemoteRefObservation::Unobservable { reason } => {
                assert!(reason.contains("no matching remote"), "{reason}")
            }
            other => panic!("a remote that is not configured cannot have answered: {other:?}"),
        }
    }

    #[test]
    fn identity_keeps_the_host_across_every_url_shape() {
        for url in [
            "https://github.com/acme/Widgets.git",
            "https://github.com/acme/widgets",
            "https://user:token@github.com/acme/widgets.git",
            "git@github.com:acme/widgets.git",
            "ssh://git@github.com/acme/widgets",
            // A pull-request page URL: the `/pull/<n>` tail is not read.
            "https://github.com/acme/widgets/pull/501",
        ] {
            assert_eq!(
                repo_identity(url).as_deref(),
                Some("github.com/acme/widgets"),
                "{url}"
            );
        }
        // Same owner/repo, different host — a *different* repository.
        assert_eq!(
            repo_identity("https://ghe.example/acme/widgets.git").as_deref(),
            Some("ghe.example/acme/widgets")
        );
        // No host to read: never a guess.
        for url in [
            "/srv/git/acme/widgets.git",
            "file:///srv/git/acme/widgets",
            "https://github.com/acme",
        ] {
            assert_eq!(repo_identity(url), None, "{url}");
        }
        // An ssh_config alias reads as its own host, which matches no real
        // one — fail closed by mismatch rather than by pretending to know.
        assert_eq!(
            repo_identity("myalias:acme/widgets.git").as_deref(),
            Some("myalias/acme/widgets")
        );
    }
}
