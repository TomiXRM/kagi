//! Repository-health reads and fixes (#358, ADR-0205).
//!
//! [`read_health_facts`] gathers what `kagi_domain::repo_health::assess`
//! needs — reads only. The two fixes are ordinary planned writes:
//!
//! - `write_commit_graph`: `git commit-graph write --reachable`. Adds a cache
//!   under the common dir's `objects/info`; refs, index and working tree are
//!   untouched, and deleting the file restores the previous state.
//! - `enable_fsmonitor`: `core.fsmonitor=true` in the repository-local
//!   config, and nothing else. `git config --unset core.fsmonitor` undoes it.
//!
//! Kagi's own git calls keep the monitor off (`run_git` passes
//! `-c core.fsmonitor=`), so enabling it only speeds up the user's own git.

use super::*;
use kagi_domain::plan_note::{MaintenanceNote, MaintenanceRecovery, MaintenanceTitle};
use kagi_domain::repo_health::HealthFacts;
use std::path::{Path, PathBuf};

/// The command a user would type for [`plan_write_commit_graph`].
pub const WRITE_COMMIT_GRAPH_COMMAND: &str = "git commit-graph write --reachable";
/// The command a user would type for [`plan_enable_fsmonitor`].
pub const ENABLE_FSMONITOR_COMMAND: &str = "git config core.fsmonitor true";

/// Whether git's built-in fsmonitor daemon exists on this platform.
const FSMONITOR_SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// The single-file commit-graph and the split-chain directory, under the
/// common dir so a linked worktree reads the same object store.
fn commit_graph_paths(repo: &Repository) -> (PathBuf, PathBuf) {
    let info = repo.commondir().join("objects").join("info");
    (info.join("commit-graph"), info.join("commit-graphs"))
}

fn mtime_secs(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(secs).ok()
}

/// Newest commit-graph file's mtime: the single file or the split chain.
fn commit_graph_mtime(repo: &Repository) -> Option<i64> {
    let (single, split) = commit_graph_paths(repo);
    mtime_secs(&single).max(mtime_secs(&split.join("commit-graph-chain")))
}

/// `core.fsmonitor` as git would see it, from any config level.
fn fsmonitor_value(repo: &Repository) -> Result<Option<String>, GitError> {
    let config = repo
        .config()
        .map_err(|e| GitError::Other(format!("config read failed: {}", e.message())))?;
    Ok(config.get_string("core.fsmonitor").ok())
}

/// Read the facts `kagi_domain::repo_health::assess` judges. Read-only.
pub fn read_health_facts(repo: &Repository) -> Result<HealthFacts, GitError> {
    let head_commit_time = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .map(|commit| commit.committer().when().seconds());
    Ok(HealthFacts {
        bare: repo.is_bare(),
        head_commit_time,
        commit_graph_mtime: commit_graph_mtime(repo),
        fsmonitor: fsmonitor_value(repo)?,
        fsmonitor_supported: FSMONITOR_SUPPORTED,
    })
}

/// The shared plan shape: nothing but the one cache / key changes.
fn health_plan(
    repo: &Repository,
    title: MaintenanceTitle,
    after: String,
    blockers: Vec<PlanNote>,
    warnings: Vec<PlanNote>,
    recovery: PlanRecovery,
    equivalent: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let predicted = StateSummary {
        head: current.head.clone(),
        dirty: format!("{after}; working tree unchanged"),
    };
    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Maintenance(title),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: Some(equivalent.to_string()),
    })
}

// ────────────────────────────────────────────────────────────
// commit-graph
// ────────────────────────────────────────────────────────────

/// Plan writing a commit-graph for every reachable commit. Blocked only when
/// there is no history yet; rewriting a current graph is harmless.
pub fn plan_write_commit_graph(repo: &Repository) -> Result<OperationPlan, GitError> {
    let mut blockers = Vec::new();
    if repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .is_none()
    {
        blockers.push(PlanNote::Maintenance(MaintenanceNote::NoCommits));
    }
    let (single, split) = commit_graph_paths(repo);
    let recovery = PlanRecovery {
        kind: RecoveryKind::Maintenance(MaintenanceRecovery::WriteCommitGraph),
        commands: vec![
            format!("rm -f '{}'", single.display()),
            format!("rm -rf '{}'", split.display()),
        ],
    };
    health_plan(
        repo,
        MaintenanceTitle::WriteCommitGraph,
        "commit-graph written for every reachable commit".to_string(),
        blockers,
        Vec::new(),
        recovery,
        WRITE_COMMIT_GRAPH_COMMAND,
    )
}

/// HEAD must not have moved since the plan (the graph describes it).
pub fn preflight_write_commit_graph(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    preflight_check(repo, plan)
}

/// A commit-graph file exists after the write.
pub fn verify_write_commit_graph(repo: &Repository) -> Result<(), GitError> {
    if commit_graph_mtime(repo).is_none() {
        return Err(GitError::Other(
            "commit-graph write exited 0 but no commit-graph file exists".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn execute_write_commit_graph(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    preflight_write_commit_graph(repo, plan)?;
    let dir = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();
    let out = run_git(&dir, &["commit-graph", "write", "--reachable"])?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "commit-graph write failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    verify_write_commit_graph(repo)
}

// ────────────────────────────────────────────────────────────
// core.fsmonitor
// ────────────────────────────────────────────────────────────

/// Why `core.fsmonitor=true` cannot be written now, if anything.
fn fsmonitor_blockers(repo: &Repository) -> Result<Vec<PlanNote>, GitError> {
    let mut blockers = Vec::new();
    if let Some(value) = fsmonitor_value(repo)? {
        blockers.push(PlanNote::Maintenance(
            MaintenanceNote::FsmonitorAlreadySet { value },
        ));
    }
    if !FSMONITOR_SUPPORTED {
        blockers.push(PlanNote::Maintenance(MaintenanceNote::FsmonitorUnsupported));
    }
    Ok(blockers)
}

/// Plan setting `core.fsmonitor=true` in the local config. Refused when the
/// key already has a value at any level (the user's choice stands) or the
/// platform has no built-in monitor.
pub fn plan_enable_fsmonitor(repo: &Repository) -> Result<OperationPlan, GitError> {
    let recovery = PlanRecovery {
        kind: RecoveryKind::Maintenance(MaintenanceRecovery::EnableFsmonitor),
        commands: vec!["git config --unset core.fsmonitor".to_string()],
    };
    health_plan(
        repo,
        MaintenanceTitle::EnableFsmonitor,
        "core.fsmonitor = true (local config)".to_string(),
        fsmonitor_blockers(repo)?,
        vec![PlanNote::Maintenance(
            MaintenanceNote::FsmonitorStartsDaemon,
        )],
        recovery,
        ENABLE_FSMONITOR_COMMAND,
    )
}

/// HEAD unmoved, and the key is still unset at every level.
pub fn preflight_enable_fsmonitor(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    match fsmonitor_blockers(repo)?.into_iter().next() {
        Some(blocker) => Err(GitError::Blocked(Box::new(blocker))),
        None => Ok(()),
    }
}

/// The repository-local config file now says `core.fsmonitor = true`, read
/// back from disk rather than through a cached config handle.
pub fn verify_enable_fsmonitor(repo: &Repository) -> Result<(), GitError> {
    let path = repo.commondir().join("config");
    let config = git2::Config::open(&path)
        .map_err(|e| GitError::Other(format!("config read failed: {}", e.message())))?;
    match config.get_bool("core.fsmonitor") {
        Ok(true) => Ok(()),
        _ => Err(GitError::Other(
            "core.fsmonitor was not written to the local config".to_string(),
        )),
    }
}

pub(crate) fn execute_enable_fsmonitor(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    preflight_enable_fsmonitor(repo, plan)?;
    let mut local = repo
        .config()
        .and_then(|config| config.open_level(git2::ConfigLevel::Local))
        .map_err(|e| GitError::Other(format!("local config open failed: {}", e.message())))?;
    local
        .set_bool("core.fsmonitor", true)
        .map_err(|e| GitError::Other(format!("config write failed: {}", e.message())))?;
    verify_enable_fsmonitor(repo)
}
