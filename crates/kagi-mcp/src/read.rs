//! Read-tool implementations (#331). All are side-effect-free: they open the
//! backend, take a `RepoSnapshot` (or read the oplog), and serialize.
//!
//! Serialization lives here at the MCP edge — the domain types stay pure. We
//! read their public fields and render small JSON objects by hand (no serde
//! derive on `kagi-domain`), matching how `cli_main.rs` does it.

use std::path::Path;

use kagi_domain::commit::{Commit, Signature};
use kagi_domain::status::ChangeKind;
use kagi_git::{Backend, FileDiffStat, FileStatus};
use serde_json::{json, Value};

type ToolResult = Result<Value, String>;

/// Default commit-graph walk bound (matches the #331 schema default).
const DEFAULT_GRAPH_LIMIT: usize = 200;
/// How many commits a snapshot walks for status/branch context.
const SNAPSHOT_LIMIT: usize = 2000;

fn open(repo: &Path) -> Result<Backend, String> {
    Backend::discover(repo).map_err(|e| e.to_string())
}

/// Dispatch a read tool by name.
pub fn call(repo: &Path, name: &str, args: &Value) -> ToolResult {
    match name {
        "kagi_repo_status" => repo_status(repo),
        "kagi_graph" => graph(repo, args),
        "kagi_diff" => diff(repo, args),
        "kagi_commit_show" => commit_show(repo, args),
        "kagi_branches" => branches(repo),
        "kagi_worktrees" => worktrees(repo),
        "kagi_conflicts" => conflicts(repo),
        "kagi_stashes" => stashes(repo),
        "kagi_oplog" => oplog(args),
        other => Err(format!("unknown read tool '{}'", other)),
    }
}

fn limit_arg(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(default)
}

// ── serialization helpers ──────────────────────────────────────

fn sig_json(s: &Signature) -> Value {
    json!({ "name": s.name, "email": s.email, "time": s.time })
}

fn change_json(change: &ChangeKind) -> Value {
    match change {
        ChangeKind::Renamed { from } => {
            json!({ "kind": "Renamed", "from": from.to_string_lossy() })
        }
        other => json!({ "kind": other.label() }),
    }
}

fn file_status_json(f: &FileStatus) -> Value {
    json!({ "path": f.path.to_string_lossy(), "change": change_json(&f.change) })
}

fn diffstat_json(f: &FileDiffStat) -> Value {
    json!({
        "path": f.path.to_string_lossy(),
        "change": change_json(&f.change),
        "additions": f.additions,
        "deletions": f.deletions,
        "is_binary": f.is_binary,
    })
}

fn commit_meta_json(c: &Commit) -> Value {
    json!({
        "sha": c.id.0,
        "parents": c.parents.iter().map(|p| p.0.clone()).collect::<Vec<_>>(),
        "author": sig_json(&c.author),
        "committer": sig_json(&c.committer),
        "summary": c.summary,
        "message": c.message,
    })
}

// ── tools ──────────────────────────────────────────────────────

fn repo_status(repo: &Path) -> ToolResult {
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    let st = &snap.status;

    // Upstream / ahead / behind come from the checked-out branch's tracking info.
    let (branch, head_sha) = match &snap.head {
        kagi_git::Head::Attached { branch, target } => (Some(branch.clone()), Some(target.clone())),
        kagi_git::Head::Detached { target } => (None, Some(target.clone())),
        kagi_git::Head::Unborn { .. } => (None, None),
    };
    let upstream = branch
        .as_ref()
        .and_then(|b| snap.branches.iter().find(|br| &br.name == b))
        .and_then(|br| br.upstream.as_ref());

    Ok(json!({
        "branch": branch,
        "head_sha": head_sha,
        "detached": matches!(snap.head, kagi_git::Head::Detached { .. }),
        "upstream": upstream.map(|u| u.remote_branch.clone()),
        "ahead": upstream.map(|u| u.ahead).unwrap_or(0),
        "behind": upstream.map(|u| u.behind).unwrap_or(0),
        "dirty": !st.staged.is_empty() || !st.unstaged.is_empty()
            || !st.untracked.is_empty() || !st.conflicted.is_empty(),
        "staged": st.staged.iter().map(file_status_json).collect::<Vec<_>>(),
        "unstaged": st.unstaged.iter().map(file_status_json).collect::<Vec<_>>(),
        "untracked": st.untracked.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        "conflicts": st.conflicted.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        "stash_count": snap.stashes.len(),
        "last_fetch_secs": snap.last_fetch_secs,
    }))
}

fn graph(repo: &Path, args: &Value) -> ToolResult {
    let limit = limit_arg(args, "limit", DEFAULT_GRAPH_LIMIT);
    let mut backend = open(repo)?;
    let snap = backend.snapshot(limit).map_err(|e| e.to_string())?;
    let rows: Vec<Value> = snap
        .commits
        .iter()
        .map(|c| {
            json!({
                "sha": c.id.0,
                "parents": c.parents.iter().map(|p| p.0.clone()).collect::<Vec<_>>(),
                "summary": c.summary,
                "author": sig_json(&c.author),
            })
        })
        .collect();
    Ok(json!({ "rows": rows }))
}

fn diff(repo: &Path, args: &Value) -> ToolResult {
    let backend = open(repo)?;
    let staged = args.get("staged").and_then(Value::as_bool).unwrap_or(false);
    let from = args.get("from").and_then(Value::as_str);
    let to = args.get("to").and_then(Value::as_str);

    if let (Some(a), Some(b)) = (from, to) {
        // Compare two revisions (file list; per-file line counts aren't provided
        // by compare_commits, so additions/deletions are omitted here).
        use kagi_domain::commit::CommitId;
        let files = backend
            .compare_commits(&CommitId(a.to_string()), &CommitId(b.to_string()))
            .map_err(|e| e.to_string())?;
        return Ok(json!({
            "mode": "range",
            "from": a, "to": b,
            "files": files.iter().map(file_status_json).collect::<Vec<_>>(),
        }));
    }

    let (mode, stats) = if staged {
        ("staged", backend.staged_diffstat())
    } else {
        ("unstaged", backend.unstaged_diffstat())
    };
    let stats = stats.map_err(|e| e.to_string())?;
    Ok(json!({
        "mode": mode,
        "files": stats.iter().map(diffstat_json).collect::<Vec<_>>(),
    }))
}

fn commit_show(repo: &Path, args: &Value) -> ToolResult {
    let revision = args
        .get("revision")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing `revision`".to_string())?;
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    // Resolve by full sha or unambiguous prefix against the snapshot's commits.
    let commit = snap
        .commits
        .iter()
        .find(|c| c.id.0 == revision || c.id.0.starts_with(revision))
        .ok_or_else(|| {
            format!(
                "revision '{}' not found in the last {} commits",
                revision, SNAPSHOT_LIMIT
            )
        })?;
    let files = backend
        .commit_diffstat(&commit.id)
        .map_err(|e| e.to_string())?;
    let mut out = commit_meta_json(commit);
    out["files"] = json!(files.iter().map(diffstat_json).collect::<Vec<_>>());
    Ok(out)
}

fn branches(repo: &Path) -> ToolResult {
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    let local: Vec<Value> = snap
        .branches
        .iter()
        .map(|b| {
            json!({
                "name": b.name,
                "target": b.target.0,
                "upstream": b.upstream.as_ref().map(|u| json!({
                    "remote_branch": u.remote_branch,
                    "ahead": u.ahead,
                    "behind": u.behind,
                })),
            })
        })
        .collect();
    let remote: Vec<Value> = snap
        .remote_branches
        .iter()
        .map(|r| json!({ "remote": r.remote, "name": r.name, "target": r.target.0 }))
        .collect();
    Ok(json!({ "local": local, "remote": remote }))
}

fn worktrees(repo: &Path) -> ToolResult {
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    let list: Vec<Value> = snap
        .worktrees
        .iter()
        .map(|w| {
            json!({
                "name": w.name,
                "path": w.path.to_string_lossy(),
                "branch": w.branch,
                "is_current": w.is_current,
                "is_main": w.is_main,
                "locked": w.locked,
                "lock_reason": w.lock_reason,
            })
        })
        .collect();
    Ok(json!({ "worktrees": list }))
}

fn conflicts(repo: &Path) -> ToolResult {
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "conflicts": snap.status.conflicted.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
    }))
}

fn stashes(repo: &Path) -> ToolResult {
    let mut backend = open(repo)?;
    let snap = backend
        .snapshot(SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    let list: Vec<Value> = snap
        .stashes
        .iter()
        .map(|s| json!({ "index": s.index, "message": s.message, "target": s.target.0 }))
        .collect();
    Ok(json!({ "stashes": list }))
}

fn oplog(args: &Value) -> ToolResult {
    let limit = limit_arg(args, "limit", 20);
    let entries = kagi_git::read_oplog_tail(limit);
    let items: Vec<Value> = entries
        .iter()
        .map(|e| serde_json::from_str(&kagi_git::entry_to_json(e)).unwrap_or(Value::Null))
        .collect();
    Ok(json!({ "entries": items }))
}
