//! Worktree inspection — occupancy + advisory removal facts (#633).
//!
//! One read-only pass over a single registered worktree:
//!
//! - **How much disk it occupies**, build output split out
//!   ([`WorktreeDiskUsage`]). `target/` is ignored by Git, so Git sees none of
//!   it, yet it is where the gigabytes are — #633 measured 148 GB of worktrees.
//! - **What the repository currently supports claiming about removing it**
//!   ([`WorktreeRemovalFacts`]), which the pure `worktree_removal_verdict`
//!   turns into an advisory verdict.
//!
//! Nothing here mutates anything, and nothing here changes what removal does:
//! the remove menu, plan, and backup behaviour are untouched, and a
//! `SafeMerged` verdict grants no extra permission. No network, no `gh`, no
//! `du` child process.
//!
//! # Trust rules
//!
//! Only `Worktree::path` is read from the caller's snapshot. Identity
//! (`is_main`), lock state, HEAD, branch, and cleanliness are re-observed from
//! a freshly opened repository, because a removal claim needs a present
//! observation, not a past one. `Worktree::wip == None` means "not read", not
//! "clean", so it is never evidence.
//!
//! Every unanswerable question becomes `Unknown` with a typed reason, never
//! `No` and never `Yes`: a failed read must not read as "nothing to lose".
//!
//! # Locality and cancellation
//!
//! `kagi-git` is the local-`git2` layer; remote-over-SSH repositories are a
//! separate read path (`src/remote`, ADR-0089) that never reaches this module,
//! so `repo_path` is always a local filesystem path.
//!
//! The whole call belongs on a background executor — a worktree with a warm
//! `target/` is hundreds of thousands of files. `cancel` is polled between
//! phases and at every entry of the traversal; a cancelled pass reports
//! `Unknown(ObservationFailed)` and an `Err` occupancy rather than a partial
//! total. Staleness is the UI's (owner/generation) business.
//!
//! Because the traversal is the long phase, the repository facts are read
//! *after* it, so they are not minutes older than the size they accompany.
//! The two halves are still separate reads, not one atomic snapshot: the
//! removal preflight remains the authoritative check before anything is
//! deleted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use git2::{ErrorCode, Repository, WorktreeLockStatus};

use kagi_domain::refs::Worktree;
use kagi_domain::remove::{WorktreeEvidence, WorktreeRemovalFacts, WorktreeUnknownReason};

use crate::CommitId;

mod disk;

pub use disk::WorktreeDiskUsage;

/// Message used for every cancelled outcome, so a cancelled pass is
/// distinguishable from a genuine failure without parsing OS error text.
const CANCELLED: &str = "inspection cancelled";

/// One worktree's read-only observation.
#[derive(Clone, Debug)]
pub struct WorktreeInspection {
    /// The inspected working-tree path, resolved.
    pub path: PathBuf,
    /// The commit this worktree's HEAD resolves to right now; `None` for an
    /// unborn HEAD or an unreadable one (`removal` says which).
    pub head: Option<CommitId>,
    /// Freshly observed facts for `worktree_removal_verdict`.
    pub removal: WorktreeRemovalFacts,
    /// Occupied disk, or why it could not be measured. A partial traversal is
    /// never reported as a total.
    pub disk_usage: Result<WorktreeDiskUsage, String>,
    /// Underlying failure detail for diagnostics. The user-facing cause is the
    /// typed [`WorktreeUnknownReason`]; these strings are raw backend text and
    /// are not meant to be localized.
    pub errors: Vec<String>,
    /// Default branch the `merged` fact was checked against, when it exists.
    pub default_branch: Option<String>,
}

/// Observe one registered worktree: occupied disk and removal-safety facts.
///
/// `repo_path` is any local path inside the repository (the main worktree or a
/// linked one); it is opened fresh and reduced to its common dir, so the
/// registry and the main working directory come from the repository itself.
///
/// Always returns a fully-populated [`WorktreeInspection`] — a failure is
/// `Unknown` evidence plus an `errors` entry, never a missing or optimistic
/// answer. Performs no writes and no network I/O.
pub fn inspect_worktree(
    repo_path: &Path,
    worktree: &Worktree,
    cancel: &AtomicBool,
) -> WorktreeInspection {
    let mut out = WorktreeInspection::unobserved(worktree.path.clone());

    if cancelled(cancel) {
        out.fail(CANCELLED.to_string());
        return out;
    }

    let main = match open_main_repository(repo_path) {
        Ok(repo) => repo,
        Err(detail) => {
            out.fail(detail);
            return out;
        }
    };

    // Identity comes from the registry, not from `worktree.is_main`: removing
    // the main worktree is never offered, so mistaking one for the other is
    // the one error this must not make.
    let identity = match resolve_identity(&main, &worktree.path) {
        Ok(identity) => identity,
        Err(detail) => {
            out.fail(detail);
            return out;
        }
    };
    out.removal.is_main = matches!(identity, Identity::Main { .. });
    out.path = identity.path().to_path_buf();

    // Occupancy first, repository facts after. The traversal is the long
    // phase, and facts read before it would be minutes older than the size
    // they are shown beside. This is no atomic snapshot — a worktree can
    // change under either half, and the removal preflight stays the
    // authoritative check before anything is deleted.
    out.disk_usage = disk::scan(&out.path, cancel);
    if let Err(detail) = &out.disk_usage {
        out.errors.push(detail.clone());
    }

    if cancelled(cancel) {
        out.cancel();
        return out;
    }

    let locked_worktree = observe_lock(&main, &identity, &mut out);

    // The worktree's own repository view: its HEAD, its index, its working tree.
    let worktree_repo = match &identity {
        Identity::Main { .. } => Repository::open(main.path()),
        Identity::Linked { path, .. } => match &locked_worktree {
            Some(handle) => Repository::open_from_worktree(handle),
            // The registry entry was unreadable above; the directory is still a
            // usable repository view, so HEAD and status remain observable.
            None => Repository::open(path),
        },
    };
    let worktree_repo = match worktree_repo {
        Ok(repo) => repo,
        Err(error) => {
            out.fail(format!(
                "{}: repository unreadable: {}",
                out.path.display(),
                error.message()
            ));
            return out;
        }
    };

    observe_cleanliness(&worktree_repo, &mut out);

    if cancelled(cancel) {
        out.cancel();
        return out;
    }

    let head = observe_head(&worktree_repo, &mut out);
    observe_merged(&main, head.as_ref(), &mut out);
    observe_pushed(&worktree_repo, head.as_ref(), &mut out);

    out
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

const fn unknown(reason: WorktreeUnknownReason) -> WorktreeEvidence {
    WorktreeEvidence::Unknown(reason)
}

impl WorktreeInspection {
    /// Nothing observed yet: every question unknown, nothing measured.
    fn unobserved(path: PathBuf) -> Self {
        let unread = unknown(WorktreeUnknownReason::ObservationFailed);
        Self {
            path,
            head: None,
            removal: WorktreeRemovalFacts {
                is_main: false,
                clean: unread,
                pushed: unread,
                merged: unread,
                unlocked: unread,
            },
            disk_usage: Err("worktree not inspected".to_string()),
            errors: Vec::new(),
            default_branch: None,
        }
    }

    /// The pass could not start or could not identify the worktree. Every fact
    /// stays `Unknown(ObservationFailed)`, so nothing reads as safe.
    fn fail(&mut self, detail: String) {
        self.disk_usage = Err(detail.clone());
        self.errors.push(detail);
    }

    /// Record raw failure detail; the caller sets the matching typed evidence.
    fn observation_failed(&mut self, detail: String) {
        self.errors.push(detail);
    }

    /// Stop mid-pass: what was observed stays, what was unknown stays unknown,
    /// and no partial disk total is reported.
    fn cancel(&mut self) {
        self.disk_usage = Err(CANCELLED.to_string());
        self.errors.push(CANCELLED.to_string());
    }
}

/// Which worktree of the repository a path turned out to be.
enum Identity {
    Main { path: PathBuf },
    Linked { name: String, path: PathBuf },
}

impl Identity {
    fn path(&self) -> &Path {
        match self {
            Identity::Main { path } | Identity::Linked { path, .. } => path,
        }
    }
}

/// Open the repository owning `repo_path` and reduce it to its main
/// repository, whose `workdir()` is the main worktree and whose registry lists
/// every linked worktree. `repo_path` may itself be a linked worktree.
fn open_main_repository(repo_path: &Path) -> Result<Repository, String> {
    let opened = Repository::open(repo_path).map_err(|error| {
        format!(
            "{}: not a readable repository: {}",
            repo_path.display(),
            error.message()
        )
    })?;
    let common = opened.commondir().to_path_buf();
    Repository::open(&common).map_err(|error| {
        format!(
            "{}: main repository unreadable: {}",
            common.display(),
            error.message()
        )
    })
}

/// Match `requested` against the main working directory and the worktree
/// registry, by resolved path. A path that is not a registered worktree is
/// refused rather than inspected: the traversal must never be pointed at an
/// arbitrary directory.
fn resolve_identity(main: &Repository, requested: &Path) -> Result<Identity, String> {
    let want = resolve_path(requested)?;

    if let Some(workdir) = main.workdir() {
        if matches!(resolve_path(workdir), Ok(path) if path == want) {
            return Ok(Identity::Main { path: want });
        }
    }

    let names = main.worktrees().map_err(|error| {
        format!(
            "{}: worktree registry unreadable: {}",
            requested.display(),
            error.message()
        )
    })?;
    for name in names.iter().filter_map(|name| name.ok().flatten()) {
        let Ok(handle) = main.find_worktree(name) else {
            continue;
        };
        // A registered worktree whose directory is gone cannot be resolved.
        // That is not an answer about *this* path, so keep looking.
        if matches!(resolve_path(handle.path()), Ok(path) if path == want) {
            return Ok(Identity::Linked {
                name: name.to_string(),
                path: want,
            });
        }
    }

    Err(format!(
        "{}: not a registered worktree of this repository",
        requested.display()
    ))
}

/// Canonical form of a path, for identity comparison only.
fn resolve_path(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// A linked worktree's lock lives in its administrative entry, so it is read
/// through the registry handle — which the caller then reuses to open the
/// worktree. The main worktree has no such entry and `git worktree lock`
/// refuses it, so it is unlockable by construction.
fn observe_lock(
    main: &Repository,
    identity: &Identity,
    out: &mut WorktreeInspection,
) -> Option<git2::Worktree> {
    let Identity::Linked { name, .. } = identity else {
        out.removal.unlocked = WorktreeEvidence::Yes;
        return None;
    };
    match main.find_worktree(name) {
        Ok(handle) => {
            out.removal.unlocked = match handle.is_locked() {
                Ok(WorktreeLockStatus::Unlocked) => WorktreeEvidence::Yes,
                Ok(WorktreeLockStatus::Locked(_)) => WorktreeEvidence::No,
                Err(error) => {
                    out.observation_failed(format!(
                        "worktree '{name}': lock state unreadable: {}",
                        error.message()
                    ));
                    unknown(WorktreeUnknownReason::ObservationFailed)
                }
            };
            Some(handle)
        }
        Err(error) => {
            out.observation_failed(format!(
                "worktree '{name}': registry entry unreadable: {}",
                error.message()
            ));
            None
        }
    }
}

/// Clean means Git reports no staged, unstaged, untracked, or conflicted path.
/// Ignored files (`target/`, `.env`) are deliberately outside this question:
/// they are the occupancy answer, and the UI warns that ignored files a user
/// cares about still need review before removal.
fn observe_cleanliness(worktree_repo: &Repository, out: &mut WorktreeInspection) {
    // The read-only status: the stat-cache-repairing variant writes the index,
    // and an inspection pass has confirmed nothing.
    match crate::status::working_tree_status(worktree_repo) {
        Ok(status) => {
            let dirty = !status.staged.is_empty()
                || !status.unstaged.is_empty()
                || !status.untracked.is_empty()
                || !status.conflicted.is_empty();
            out.removal.clean = if dirty {
                WorktreeEvidence::No
            } else {
                WorktreeEvidence::Yes
            };
        }
        Err(error) => {
            out.observation_failed(format!(
                "{}: working tree status unreadable: {error:?}",
                out.path.display()
            ));
            out.removal.clean = unknown(WorktreeUnknownReason::ObservationFailed);
        }
    }
}

/// HEAD reduced to what the two history questions need.
struct HeadState {
    oid: git2::Oid,
    branch: HeadBranch,
}

/// What HEAD is attached to, which decides whether an upstream can exist.
enum HeadBranch {
    /// Full refname of the checked-out branch.
    Ref(String),
    /// No branch, so no upstream can be configured for it.
    Detached,
    /// A branch whose refname could not be read — ignorance, not detachment.
    Unreadable,
}

/// HEAD as the worktree itself resolves it. `None` for an unborn or unreadable
/// HEAD, recording which: "no commit yet" and "could not read" are different
/// answers to every later question.
fn observe_head(worktree_repo: &Repository, out: &mut WorktreeInspection) -> Option<HeadState> {
    match worktree_repo.head() {
        Ok(reference) => match reference.target() {
            Some(oid) => {
                out.head = Some(CommitId(oid.to_string()));
                Some(HeadState {
                    oid,
                    branch: head_branch(&reference, out),
                })
            }
            None => {
                out.observation_failed(format!(
                    "{}: HEAD resolves to no commit",
                    out.path.display()
                ));
                None
            }
        },
        Err(error) if error.code() == ErrorCode::UnbornBranch => {
            out.removal.merged = unknown(WorktreeUnknownReason::Unborn);
            out.removal.pushed = unknown(WorktreeUnknownReason::Unborn);
            None
        }
        Err(error) => {
            out.observation_failed(format!(
                "{}: HEAD unreadable: {}",
                out.path.display(),
                error.message()
            ));
            None
        }
    }
}

/// Classify HEAD's attachment. `Reference::name` fails only on a non-UTF-8
/// refname, which cannot be turned into an upstream query and therefore must
/// not be reported as "no upstream".
fn head_branch(reference: &git2::Reference<'_>, out: &mut WorktreeInspection) -> HeadBranch {
    if !reference.is_branch() {
        return HeadBranch::Detached;
    }
    match reference.name() {
        Ok(name) => HeadBranch::Ref(name.to_string()),
        Err(error) => {
            out.observation_failed(format!(
                "{}: checked-out branch refname unreadable: {}",
                out.path.display(),
                error.message()
            ));
            HeadBranch::Unreadable
        }
    }
}

/// Merged = the head commit is contained in the default branch's history, the
/// same question as `git merge-base --is-ancestor`. Detached HEADs answer this
/// perfectly well; it is only `pushed` they cannot answer.
///
/// The default branch is resolved by the existing branch-cleanup policy
/// (`origin/HEAD`, else `main`/`master`), so removal safety and cleanup agree
/// on what "the default branch" means.
fn observe_merged(main: &Repository, head: Option<&HeadState>, out: &mut WorktreeInspection) {
    let name = crate::ops::default_branch_name(main);
    let Some(tip) = crate::ops::resolve_main_tip(main, &name) else {
        // Nothing to compare against: an unborn or exotic repository. An
        // already-typed reason (unborn HEAD) must not be overwritten.
        if head.is_some() {
            out.observation_failed(format!(
                "default branch '{name}' has no resolvable tip; merged is unknown"
            ));
            out.removal.merged = unknown(WorktreeUnknownReason::ObservationFailed);
        }
        return;
    };
    out.default_branch = Some(name);

    let Some(head) = head else {
        return;
    };
    out.removal.merged = contains(main, tip, head.oid, "the default branch", out);
}

/// Pushed = the head commit is contained in the branch's **remote-tracking**
/// ref, read locally. No network request is made, so this proves only what the
/// last fetch/push left behind — the UI discloses that basis.
///
/// Four distinctions this keeps:
/// - No upstream configured (or a detached HEAD, which can have none) is a
///   positive observation: `No`.
/// - An upstream configured whose remote-tracking ref is missing or unreadable
///   is ignorance: `Unknown(UpstreamUnavailable)`.
/// - `branch.<name>.remote = .` is a *local* upstream, never proof of
///   publication, so it reads as `No` regardless of what it contains.
/// - An upstream that resolves to something outside `refs/remotes/` is also
///   ignorance, not proof. A refspec like
///   `remote.origin.fetch = +refs/heads/*:refs/heads/*` makes libgit2 answer
///   `refs/heads/feat` — the local branch itself — so reading its tip would
///   call every unpublished commit pushed. The same trap hides one level down:
///   a `refs/remotes/` ref that is only a symbolic alias for a local branch is
///   no better, so the ref is resolved and its target name checked as well.
fn observe_pushed(
    worktree_repo: &Repository,
    head: Option<&HeadState>,
    out: &mut WorktreeInspection,
) {
    let Some(head) = head else {
        return;
    };
    let branch_ref = match &head.branch {
        HeadBranch::Ref(name) => name.as_str(),
        HeadBranch::Detached => {
            out.removal.pushed = WorktreeEvidence::No;
            return;
        }
        HeadBranch::Unreadable => {
            out.removal.pushed = unknown(WorktreeUnknownReason::ObservationFailed);
            return;
        }
    };

    let remote = match worktree_repo.branch_upstream_remote(branch_ref) {
        Ok(buf) => match buf.as_str() {
            Ok(name) => name.trim().to_string(),
            Err(_) => {
                out.observation_failed(format!(
                    "{branch_ref}: configured upstream remote name is not UTF-8"
                ));
                out.removal.pushed = unknown(WorktreeUnknownReason::ObservationFailed);
                return;
            }
        },
        Err(error) if error.code() == ErrorCode::NotFound => {
            out.removal.pushed = WorktreeEvidence::No;
            return;
        }
        Err(error) => {
            out.observation_failed(format!(
                "{branch_ref}: upstream remote unreadable: {}",
                error.message()
            ));
            out.removal.pushed = unknown(WorktreeUnknownReason::ObservationFailed);
            return;
        }
    };
    if remote.is_empty() || remote == "." {
        out.removal.pushed = WorktreeEvidence::No;
        return;
    }

    let upstream_ref = match worktree_repo.branch_upstream_name(branch_ref) {
        Ok(buf) => match buf.as_str() {
            Ok(name) => name.to_string(),
            Err(_) => {
                out.observation_failed(format!("{branch_ref}: upstream ref name is not UTF-8"));
                out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
                return;
            }
        },
        Err(error) if error.code() == ErrorCode::NotFound => {
            out.observation_failed(format!(
                "{branch_ref}: upstream '{remote}' is configured but has no remote-tracking ref"
            ));
            out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
            return;
        }
        Err(error) => {
            out.observation_failed(format!(
                "{branch_ref}: upstream ref unreadable: {}",
                error.message()
            ));
            out.removal.pushed = unknown(WorktreeUnknownReason::ObservationFailed);
            return;
        }
    };
    if !is_remote_tracking(&upstream_ref) {
        out.observation_failed(format!(
            "{branch_ref}: upstream '{remote}' maps to '{upstream_ref}', \
             which is not a remote-tracking ref; publication is unknown"
        ));
        out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
        return;
    }

    // `refs/remotes/origin/HEAD` is a legitimate symbolic ref, so resolve first
    // and judge what it actually lands on: a symbolic ref pointing back into
    // `refs/heads/` is a local branch wearing a remote name.
    let resolved = worktree_repo
        .find_reference(&upstream_ref)
        .and_then(|reference| reference.resolve());
    let Ok(resolved) = resolved else {
        out.observation_failed(format!(
            "{upstream_ref}: remote-tracking ref missing or unresolvable"
        ));
        out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
        return;
    };
    match resolved.name() {
        Ok(name) if is_remote_tracking(name) => {}
        Ok(name) => {
            out.observation_failed(format!(
                "{upstream_ref}: resolves to '{name}', which is not a remote-tracking ref; \
                 publication is unknown"
            ));
            out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
            return;
        }
        Err(error) => {
            out.observation_failed(format!(
                "{upstream_ref}: resolved refname unreadable: {}",
                error.message()
            ));
            out.removal.pushed = unknown(WorktreeUnknownReason::ObservationFailed);
            return;
        }
    }

    let Some(upstream_tip) = resolved.target() else {
        out.observation_failed(format!(
            "{upstream_ref}: remote-tracking ref missing or unresolvable"
        ));
        out.removal.pushed = unknown(WorktreeUnknownReason::UpstreamUnavailable);
        return;
    };

    out.removal.pushed = contains(worktree_repo, upstream_tip, head.oid, &upstream_ref, out);
}

/// Only refs the remote populated can prove publication. Everything else —
/// `refs/heads/`, `refs/tags/`, an exotic namespace — is local state that a
/// refspec or a symbolic ref can point an upstream at.
fn is_remote_tracking(name: &str) -> bool {
    name.starts_with("refs/remotes/")
}

/// Whether `tip`'s history contains `commit` — the shared half of both history
/// questions. An unreadable graph is ignorance, not a negative answer.
fn contains(
    repo: &Repository,
    tip: git2::Oid,
    commit: git2::Oid,
    what: &str,
    out: &mut WorktreeInspection,
) -> WorktreeEvidence {
    if tip == commit {
        return WorktreeEvidence::Yes;
    }
    match repo.graph_descendant_of(tip, commit) {
        Ok(true) => WorktreeEvidence::Yes,
        Ok(false) => WorktreeEvidence::No,
        Err(error) => {
            out.observation_failed(format!(
                "ancestry of {commit} in {what} unreadable: {}",
                error.message()
            ));
            unknown(WorktreeUnknownReason::ObservationFailed)
        }
    }
}

#[cfg(test)]
#[path = "worktree_inspection/git_tests.rs"]
mod git_tests;
