//! PR merge-lifecycle backend (#347): `gh` version detection, `mergeStateStatus`
//! + merge-queue position via `gh api graphql`, and enqueue/dequeue mutations.
//!
//! Split out of `github.rs` on the merge-lifecycle feature boundary (that file
//! is at its LOC ceiling). Re-exported from `github` so the public path stays
//! `kagi_git::github::*`. Read-only except the enqueue/dequeue mutations, which
//! the UI gates behind confirmation.

use std::path::Path;
use std::sync::OnceLock;

use crate::GitError;

pub use kagi_domain::merge_state::{
    BypassCapability, MergeQueueEntryState, MergeStateStatus, MissingRequirements, QueuePosition,
};

// ── gh version detection (#347 §5: degrade gracefully, never hard-require) ──

/// Parse the version out of `gh --version` output.
///
/// The first line is `gh version 2.97.0 (2024-…)`; older/newer builds vary the
/// tail but always start with `gh version <semver>`. Returns `(major, minor,
/// patch)`, or `None` if the shape is unfamiliar (→ treat as "assume missing
/// features", the safe default).
pub fn parse_gh_version(out: &str) -> Option<(u32, u32, u32)> {
    let ver = out.split_whitespace().nth(2)?;
    let mut it = ver.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    // Patch may carry a suffix on nightly builds; take the leading digits.
    let patch = it
        .next()
        .map(|p| {
            p.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// The installed `gh` version, probed once per process.
pub fn gh_version() -> Option<(u32, u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let out = crate::cli::gh_command().arg("--version").output().ok()?;
        parse_gh_version(&String::from_utf8_lossy(&out.stdout))
    })
}

/// Whether the installed `gh` is at least `major.minor`. `false` when `gh` is
/// missing or its version could not be parsed — callers hide the version-gated
/// feature and show a note rather than failing (#347 §5, PM-locked: no hard
/// 2.99 requirement).
pub fn gh_at_least(major: u32, minor: u32) -> bool {
    match gh_version() {
        Some((maj, min, _)) => (maj, min) >= (major, minor),
        None => false,
    }
}

// ── mergeStateStatus + merge queue (via `gh api graphql`) ──────────────────

/// Everything the PR merge-status surface needs beyond the PR list: the
/// `mergeStateStatus`, this PR's merge-queue entry (if any), the unresolved
/// review-thread count, and the PR's GraphQL node id (needed to enqueue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrMergeStatus {
    pub node_id: String,
    pub state: MergeStateStatus,
    pub queue: Option<QueuePosition>,
    pub unresolved_threads: u32,
}

const MERGE_STATUS_QUERY: &str = "\
query($owner:String!,$name:String!,$number:Int!){\
 repository(owner:$owner,name:$name){\
  pullRequest(number:$number){\
   id mergeStateStatus\
   reviewThreads(first:100){nodes{isResolved}}\
   mergeQueueEntry{position estimatedTimeToMerge state\
    mergeQueue{nextEntryEstimatedTimeToMerge}}}}}";

/// Fetch `mergeStateStatus` + merge-queue position for one PR. `gh api graphql`
/// (same auth as everything else here). Read-only.
pub fn pr_merge_status(workdir: &Path, number: u64) -> Result<PrMergeStatus, GitError> {
    let (owner, name) = repo_owner_name(workdir)?;
    let out = crate::cli::gh_command()
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={MERGE_STATUS_QUERY}"),
            "-F",
            &format!("owner={owner}"),
            "-F",
            &format!("name={name}"),
            "-F",
            &format!("number={number}"),
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Err(GitError::Other(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    parse_merge_status(&String::from_utf8_lossy(&out.stdout))
}

/// `owner`/`name` for the repo at `workdir`, via `gh repo view`.
fn repo_owner_name(workdir: &Path) -> Result<(String, String), GitError> {
    let out = crate::cli::gh_command()
        .args([
            "repo",
            "view",
            "--json",
            "owner,name",
            "-q",
            ".owner.login+\"/\"+.name",
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Err(GitError::Other("not a GitHub repo".into()));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    s.split_once('/')
        .map(|(o, n)| (o.to_string(), n.to_string()))
        .ok_or_else(|| GitError::Other("unexpected repo view output".into()))
}

/// Parse the `gh api graphql` merge-status response. Pure; unit-tested. A
/// missing `mergeQueueEntry` (non-MQ repo, or not queued) yields `queue: None`
/// so the UI hides the queue section — nothing to break.
pub fn parse_merge_status(json: &str) -> Result<PrMergeStatus, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let pr = v
        .pointer("/data/repository/pullRequest")
        .ok_or_else(|| GitError::Other("no pullRequest in response".into()))?;
    let state = MergeStateStatus::from_graphql(
        pr.get("mergeStateStatus")
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    let unresolved_threads = pr
        .pointer("/reviewThreads/nodes")
        .and_then(|x| x.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .filter(|n| {
                    !n.get("isResolved")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(true)
                })
                .count() as u32
        })
        .unwrap_or(0);
    let queue = pr.get("mergeQueueEntry").filter(|e| !e.is_null()).map(|e| {
        let s = |k: &str| e.get(k).and_then(|x| x.as_str()).map(str::to_string);
        QueuePosition {
            position: e.get("position").and_then(|x| x.as_u64()),
            estimated_time_to_merge: s("estimatedTimeToMerge"),
            state: MergeQueueEntryState::from_graphql(
                e.get("state").and_then(|x| x.as_str()).unwrap_or(""),
            ),
            next_entry_estimated_time_to_merge: e
                .pointer("/mergeQueue/nextEntryEstimatedTimeToMerge")
                .and_then(|x| x.as_str())
                .map(str::to_string),
        }
    });
    Ok(PrMergeStatus {
        node_id: pr
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        state,
        queue,
        unresolved_threads,
    })
}

/// Build the `gh api graphql` args for enqueue/dequeue. Pure so the mutation
/// shape (and the `jump` flag that jumps the queue) is testable without `gh`.
///
/// `jump`/solo are queue-cutting operations the UI gates behind a **second**
/// confirmation (they reorder other people's PRs); the backend just carries
/// the flag it is given.
pub fn enqueue_args(node_id: &str, jump: bool) -> Vec<String> {
    let mutation = "mutation($id:ID!,$jump:Boolean!){\
        enqueuePullRequest(input:{pullRequestId:$id,jump:$jump}){\
         mergeQueueEntry{position}}}";
    vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={mutation}"),
        "-F".into(),
        format!("id={node_id}"),
        "-F".into(),
        format!("jump={jump}"),
    ]
}

/// Args for removing this PR from the merge queue.
pub fn dequeue_args(node_id: &str) -> Vec<String> {
    let mutation = "mutation($id:ID!){\
        dequeuePullRequest(input:{pullRequestId:$id}){\
         mergeQueueEntry{position}}}";
    vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={mutation}"),
        "-F".into(),
        format!("id={node_id}"),
    ]
}

/// Add this PR to the repository's merge queue (`enqueuePullRequest`).
/// `jump = true` cuts to the front — the UI must have taken a second
/// confirmation first.
pub fn enqueue_pr(workdir: &Path, node_id: &str, jump: bool) -> Result<String, GitError> {
    run_gh(workdir, &enqueue_args(node_id, jump))
}

/// Remove this PR from the merge queue (`dequeuePullRequest`).
pub fn dequeue_pr(workdir: &Path, node_id: &str) -> Result<String, GitError> {
    run_gh(workdir, &dequeue_args(node_id))
}

fn run_gh(workdir: &Path, args: &[String]) -> Result<String, GitError> {
    let out = crate::cli::gh_command()
        .args(args)
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gh_versions() {
        assert_eq!(
            parse_gh_version("gh version 2.97.0 (2024-11-06)"),
            Some((2, 97, 0))
        );
        assert_eq!(parse_gh_version("gh version 2.99.1"), Some((2, 99, 1)));
        assert_eq!(parse_gh_version("gh version 3.0"), Some((3, 0, 0)));
        assert_eq!(parse_gh_version("garbage"), None);
    }

    /// Merge-queue present: the entry parses into a `QueuePosition`.
    #[test]
    fn parses_merge_status_with_queue() {
        let json = r#"{"data":{"repository":{"pullRequest":{
          "id":"PR_kw1","mergeStateStatus":"BLOCKED",
          "reviewThreads":{"nodes":[{"isResolved":false},{"isResolved":true},{"isResolved":false}]},
          "mergeQueueEntry":{"position":3,"estimatedTimeToMerge":"about 6 minutes","state":"QUEUED",
            "mergeQueue":{"nextEntryEstimatedTimeToMerge":"about 2 minutes"}}}}}}"#;
        let s = parse_merge_status(json).unwrap();
        assert_eq!(s.node_id, "PR_kw1");
        assert_eq!(s.state, MergeStateStatus::Blocked);
        assert_eq!(s.unresolved_threads, 2);
        let q = s.queue.expect("queued");
        assert_eq!(q.position, Some(3));
        assert_eq!(q.state, MergeQueueEntryState::Queued);
        assert_eq!(
            q.next_entry_estimated_time_to_merge.as_deref(),
            Some("about 2 minutes")
        );
    }

    /// Acceptance §6: merge queue absent (non-MQ repo) ⇒ `queue: None`, and the
    /// rest still parses. Nothing to render, nothing to break.
    #[test]
    fn parses_merge_status_without_queue() {
        let json = r#"{"data":{"repository":{"pullRequest":{
          "id":"PR_x","mergeStateStatus":"CLEAN",
          "reviewThreads":{"nodes":[]},"mergeQueueEntry":null}}}}"#;
        let s = parse_merge_status(json).unwrap();
        assert_eq!(s.state, MergeStateStatus::Clean);
        assert!(s.queue.is_none());
        assert_eq!(s.unresolved_threads, 0);
    }

    #[test]
    fn enqueue_dequeue_args_carry_jump_and_id() {
        let jump = enqueue_args("PR_1", true);
        assert!(jump.iter().any(|a| a == "jump=true"));
        assert!(jump.iter().any(|a| a == "id=PR_1"));
        let normal = enqueue_args("PR_1", false);
        assert!(normal.iter().any(|a| a == "jump=false"));
        let dq = dequeue_args("PR_1");
        assert!(dq.iter().any(|a| a.contains("dequeuePullRequest")));
        assert!(dq.iter().any(|a| a == "id=PR_1"));
    }
}
