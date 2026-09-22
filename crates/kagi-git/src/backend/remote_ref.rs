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
    /// The local half of PR cleanup, bound to the approved worktree and tip.
    /// A merged PR cannot prove that this ref was deleted (#705).
    LocalBranch {
        branch: kagi_domain::plan::PrMergeLocalBranch,
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

fn owner_repo(host: &str, path: &str) -> Option<String> {
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

impl Backend {
    /// Freeze local cleanup before the PR merge confirmation is shown.
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

    /// Observe only the local repository named by the frozen merge approval.
    pub fn read_pr_merge_local_branch(
        &self,
        branch: &kagi_domain::plan::PrMergeLocalBranch,
    ) -> Result<Option<String>, GitError> {
        if self.write_worktree_id()? != branch.worktree {
            return Err(GitError::Other(
                "local branch repository identity changed".into(),
            ));
        }
        self.local_branch_tip(&branch.name)
    }

    /// Revalidate the frozen input, then use the existing locked delete family.
    pub(crate) fn execute_pr_merge_local_branch(
        &self,
        branch: &kagi_domain::plan::PrMergeLocalBranch,
        pr_head: &str,
        backup_refs: &mut Vec<String>,
    ) -> Result<kagi_domain::operation::PrMergeLocalOutcome, GitError> {
        use kagi_domain::operation::PrMergeLocalOutcome;
        self.require_trust()?;
        let live = self.read_pr_merge_local_branch(branch)?;
        if live != branch.tip {
            return Err(GitError::Other(
                "local branch changed after approval; not deleted".into(),
            ));
        }
        let Some(tip) = &branch.tip else {
            return Ok(PrMergeLocalOutcome::Absent {
                name: branch.name.clone(),
            });
        };
        if tip != pr_head {
            return Err(GitError::Other(
                "local branch tip differs from the approved PR head".into(),
            ));
        }
        let plan = self.plan_delete_branch(&branch.name)?;
        if plan.head_at_plan != branch.head {
            return Err(GitError::Other(
                "HEAD changed after approval; local branch not deleted".into(),
            ));
        }
        if let Some(blocker) = plan.blockers.first() {
            return Err(GitError::Blocked(Box::new(blocker.clone())));
        }
        let expected = match plan.recovery.as_ref().map(|r| &r.kind) {
            Some(kagi_domain::plan_note::RecoveryKind::Branch(
                kagi_domain::plan_note::BranchRecovery::DeleteBranch { tip, .. },
            )) => tip.as_ref(),
            _ => None,
        };
        if expected != Some(tip) {
            return Err(GitError::Other(
                "local branch changed during preflight; not deleted".into(),
            ));
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
        // PR merge may promise both remote and local cleanup. Every effect
        // must be read; a fork has no deletable head in the base repository.
        if op == "pr-merge" {
            if let Some(RecoveryKind::Github(GithubRecovery::MergePr {
                number,
                base_repo,
                delete_branch,
                cross_repository,
                local_branch,
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
                if let Some(branch) = local_branch {
                    expectations.push(RemoteExpectation::LocalBranch {
                        branch: branch.as_ref().clone(),
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

#[cfg(test)]
mod tests {
    use super::repo_identity;

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
