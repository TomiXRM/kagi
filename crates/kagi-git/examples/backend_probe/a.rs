//! P4 read-path probes for #627.
//!
//! The registry owns dispatch. This operation owns its read-path comparison and
//! retains the libgit2 handle for one timed warm series.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use git2::Repository;
use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{
    run_git_with_options, snapshot, working_tree_status, ChangeKind, FileStatus, FsmonitorMode,
    GitCliOptions, GitCliOutput, WorkingTreeStatus as DomainWorkingTreeStatus,
};
use serde::Serialize;
use serde_json::{json, Value};

struct PreparedRepository {
    path: PathBuf,
    repository: Repository,
}

static PREPARED_REPOSITORY: LazyLock<Mutex<Option<PreparedRepository>>> =
    LazyLock::new(|| Mutex::new(None));

/// A1: compare Kagi's libgit2 status read against parsed porcelain v2 output.
pub struct WorkingTreeStatus;

impl ProbeOperation for WorkingTreeStatus {
    fn name(&self) -> &'static str {
        "working-tree-status"
    }

    fn mutates_fixture(&self) -> bool {
        false
    }

    fn prepare_series(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        prepare_index_primed_series(context)
    }

    fn finish_series(&self) {
        finish_prepared_series();
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        match context.backend() {
            "libgit2" => execute_libgit2(context),
            "cli" => execute_cli(context),
            backend => Err(harness_error(format!(
                "working-tree-status requires backend libgit2 or cli, got {backend}"
            ))),
        }
    }
}

/// A2: compare the complete libgit2 snapshot with a hardened CLI composition.
pub struct Snapshot;

impl ProbeOperation for Snapshot {
    fn name(&self) -> &'static str {
        "snapshot"
    }

    fn mutates_fixture(&self) -> bool {
        false
    }

    fn prepare_series(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        prepare_index_primed_series(context)
    }

    fn finish_series(&self) {
        finish_prepared_series();
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        match context.backend() {
            "libgit2" => execute_snapshot_libgit2(context),
            "cli" => execute_snapshot_cli(context),
            backend => Err(harness_error(format!(
                "snapshot requires backend libgit2 or cli, got {backend}"
            ))),
        }
    }
}

fn prepare_index_primed_series(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let prime_start = Instant::now();
    let prime = run_git_with_options(
        context.repo(),
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--renames",
        ],
        GitCliOptions {
            executable: context.git_executable(),
            fsmonitor: FsmonitorMode::Disabled,
        },
    )
    .map_err(git_error)?;
    if prime.status != 0 {
        return Err(harness_error(format!(
            "index-prime status exited {}: {}",
            prime.status,
            prime.stderr.trim()
        )));
    }
    let index_prime_ns = prime_start.elapsed().as_nanos();
    if context.backend() != "libgit2" {
        return Ok(json!({ "index_prime_ns": index_prime_ns }));
    }

    let start = Instant::now();
    let repository = Repository::open(context.repo())?;
    let repository_open_ns = start.elapsed().as_nanos();
    let mut prepared = PREPARED_REPOSITORY
        .lock()
        .map_err(|_| harness_error("libgit2 warm-series repository lock poisoned"))?;
    *prepared = Some(PreparedRepository {
        path: context.repo().to_path_buf(),
        repository,
    });
    Ok(json!({
        "index_prime_ns": index_prime_ns,
        "repository_open_ns": repository_open_ns,
    }))
}

fn finish_prepared_series() {
    if let Ok(mut prepared) = PREPARED_REPOSITORY.lock() {
        *prepared = None;
    }
}

fn execute_snapshot_libgit2(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let mut prepared = PREPARED_REPOSITORY
        .lock()
        .map_err(|_| harness_error("libgit2 warm-series repository lock poisoned"))?;
    let repository = prepared
        .as_mut()
        .filter(|prepared| prepared.path == context.repo())
        .ok_or_else(|| harness_error("libgit2 warm-series repository does not match probe copy"))?;
    let start = Instant::now();
    let snapshot = snapshot(&mut repository.repository, 10_000).map_err(git_error)?;
    let wall_ns = start.elapsed().as_nanos();
    Ok(json!({
        "backend_path": "libgit2",
        "git_processes": 0,
        "total_ns": wall_ns,
        "stage_count": 9,
        "stages": [{ "name": "snapshot", "wall_ns": wall_ns }],
        "most_expensive_stage": "snapshot",
        "missing_information": [],
        "timing_note": "snapshot's nine internal stages remain encapsulated; this is the public snapshot() total",
        "information": {
            "commits": snapshot.commits.len(),
            "branches": snapshot.branches.len(),
            "remote_branches": snapshot.remote_branches.len(),
            "tags": snapshot.tags.len(),
            "stashes": snapshot.stashes.len(),
            "worktrees": snapshot.worktrees.len(),
            "last_fetch_secs": snapshot.last_fetch_secs,
            "status": canonical_status(&snapshot.status)?,
        },
    }))
}

fn execute_snapshot_cli(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let mut stages = Vec::with_capacity(10);
    let mut total_ns = 0u128;
    let (status_stage, status_output) = snapshot_cli_stage(
        context,
        "status",
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--renames",
        ],
    )?;
    total_ns += stage_wall_ns(&status_stage);
    if status_output.status != 0 {
        return Err(harness_error(format!(
            "snapshot status exited {}: {}",
            status_output.status,
            status_output.stderr.trim()
        )));
    }
    let status = parse_porcelain_v2(context.repo(), &status_output.stdout)?;
    stages.push(status_stage);

    for (name, args) in [
        (
            "head-symbolic-ref",
            &[
                "--no-optional-locks",
                "symbolic-ref",
                "--quiet",
                "--short",
                "HEAD",
            ][..],
        ),
        (
            "head-oid",
            &["--no-optional-locks", "rev-parse", "--verify", "HEAD"][..],
        ),
        (
            "worktrees",
            &["--no-optional-locks", "worktree", "list", "--porcelain"][..],
        ),
        (
            "commits",
            &[
                "--no-optional-locks",
                "rev-list",
                "--parents",
                "--all",
                "--max-count=10000",
            ][..],
        ),
        (
            "branches",
            &[
                "--no-optional-locks",
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:trackshort)",
                "refs/heads",
            ][..],
        ),
        (
            "remote-branches",
            &[
                "--no-optional-locks",
                "for-each-ref",
                "--format=%(refname)%00%(objectname)",
                "refs/remotes",
            ][..],
        ),
        (
            "tags",
            &[
                "--no-optional-locks",
                "for-each-ref",
                "--format=%(refname)%00%(objecttype)%00%(objectname)",
                "refs/tags",
            ][..],
        ),
        (
            "stashes",
            &[
                "--no-optional-locks",
                "stash",
                "list",
                "--format=%H%x00%gd%x00%gs",
            ][..],
        ),
    ] {
        let (stage, _) = snapshot_cli_stage(context, name, args)?;
        total_ns += stage_wall_ns(&stage);
        stages.push(stage);
    }

    let fetch_head_start = Instant::now();
    let last_fetch_secs = std::fs::metadata(context.repo().join(".git").join("FETCH_HEAD"))
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let fetch_head_wall_ns = fetch_head_start.elapsed().as_nanos();
    total_ns += fetch_head_wall_ns;
    stages.push(json!({
        "name": "fetch-head-mtime",
        "git_processes": 0,
        "wall_ns": fetch_head_wall_ns,
        "last_fetch_secs": last_fetch_secs,
    }));

    let most_expensive_stage = stages
        .iter()
        .max_by_key(|stage| stage_wall_ns(stage))
        .and_then(|stage| stage["name"].as_str())
        .unwrap_or("none");
    Ok(json!({
        "backend_path": "cli",
        "git_processes": 9,
        "total_ns": total_ns,
        "stage_count": 9,
        "stages": stages,
        "most_expensive_stage": most_expensive_stage,
        "missing_information": [
            "linked-worktree common-dir FETCH_HEAD mtime",
            "detached worktree roots outside the 10,000 commit budget",
            "annotated-tag target peeling",
            "linked-worktree WIP counts",
        ],
        "information": {
            "status": status,
            "last_fetch_secs": last_fetch_secs,
        },
    }))
}

fn snapshot_cli_stage(
    context: &ProbeContext<'_>,
    name: &'static str,
    args: &[&str],
) -> Result<(Value, GitCliOutput), HarnessError> {
    let start = Instant::now();
    let output = run_git_with_options(
        context.repo(),
        args,
        GitCliOptions {
            executable: context.git_executable(),
            fsmonitor: FsmonitorMode::Disabled,
        },
    )
    .map_err(git_error)?;
    let stage = json!({
        "name": name,
        "git_processes": 1,
        "wall_ns": start.elapsed().as_nanos(),
        "exit_status": output.status,
        "stdout_bytes": output.stdout.len(),
        "stderr_bytes": output.stderr.len(),
    });
    Ok((stage, output))
}

fn stage_wall_ns(stage: &Value) -> u128 {
    stage["wall_ns"].as_u64().unwrap_or_default().into()
}

fn execute_libgit2(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let prepared = PREPARED_REPOSITORY
        .lock()
        .map_err(|_| harness_error("libgit2 warm-series repository lock poisoned"))?;
    let repository = prepared
        .as_ref()
        .filter(|prepared| prepared.path == context.repo())
        .ok_or_else(|| harness_error("libgit2 warm-series repository does not match probe copy"))?;
    let status = working_tree_status(&repository.repository).map_err(git_error)?;
    Ok(json!({
        "backend_path": "libgit2",
        "git_processes": 0,
        "canonical": canonical_status(&status)?,
    }))
}

fn execute_cli(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let candidate = context.candidate().unwrap_or("no-optional-locks");
    let mut args = Vec::new();
    let fsmonitor = match candidate {
        "no-optional-locks" => {
            args.push("--no-optional-locks");
            FsmonitorMode::Disabled
        }
        "no-optional-locks-fsmonitor" => {
            args.push("--no-optional-locks");
            FsmonitorMode::EnabledBuiltin
        }
        "bare" => FsmonitorMode::Disabled,
        _ => {
            return Err(harness_error(format!(
                "working-tree-status has no CLI candidate {candidate:?}"
            )));
        }
    };
    args.extend([
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--renames",
    ]);
    let out = run_git_with_options(
        context.repo(),
        &args,
        GitCliOptions {
            executable: context.git_executable(),
            fsmonitor,
        },
    )
    .map_err(git_error)?;
    if out.status != 0 {
        return Err(harness_error(format!(
            "git status exited {}: {}",
            out.status,
            out.stderr.trim()
        )));
    }
    Ok(json!({
        "backend_path": "cli",
        "git_processes": 1,
        "candidate": candidate,
        "fsmonitor": match fsmonitor {
            FsmonitorMode::Disabled => "disabled",
            FsmonitorMode::EnabledBuiltin => "builtin",
        },
        "canonical": parse_porcelain_v2(context.repo(), &out.stdout)?,
    }))
}

fn require_log_dir() -> Result<(), HarnessError> {
    std::env::var_os("KAGI_LOG_DIR")
        .is_some()
        .then_some(())
        .ok_or_else(|| harness_error("KAGI_LOG_DIR must be set for #627 probes"))
}

fn git_error(error: kagi_git::GitError) -> HarnessError {
    harness_error(error.to_string())
}

fn harness_error(message: impl AsRef<str>) -> HarnessError {
    git2::Error::from_str(message.as_ref()).into()
}

#[derive(Serialize)]
struct StatusEntry {
    group: &'static str,
    path: String,
    kind: &'static str,
    rename_from: Option<String>,
}

fn canonical_status(status: &DomainWorkingTreeStatus) -> Result<Value, HarnessError> {
    let mut entries = Vec::new();
    for file in &status.staged {
        entries.push(file_entry("staged", file));
    }
    for file in &status.unstaged {
        entries.push(file_entry("unstaged", file));
    }
    for path in &status.untracked {
        entries.push(StatusEntry {
            group: "untracked",
            path: path.to_string_lossy().replace('\\', "/"),
            kind: "untracked",
            rename_from: None,
        });
    }
    for path in &status.conflicted {
        entries.push(StatusEntry {
            group: "conflicted",
            path: path.to_string_lossy().replace('\\', "/"),
            kind: "conflicted",
            rename_from: None,
        });
    }
    canonical_entries(entries)
}

fn file_entry(group: &'static str, file: &FileStatus) -> StatusEntry {
    let (kind, rename_from) = match &file.change {
        ChangeKind::Added => ("added", None),
        ChangeKind::Modified => ("modified", None),
        ChangeKind::Deleted => ("deleted", None),
        ChangeKind::Renamed { from } => {
            ("renamed", Some(from.to_string_lossy().replace('\\', "/")))
        }
        ChangeKind::TypeChange => ("type-change", None),
    };
    StatusEntry {
        group,
        path: file.path.to_string_lossy().replace('\\', "/"),
        kind,
        rename_from,
    }
}

fn parse_porcelain_v2(repo: &Path, output: &str) -> Result<Value, HarnessError> {
    let mut entries = Vec::new();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        if record.is_empty() || record.starts_with('#') {
            continue;
        }
        if let Some(rest) = record.strip_prefix("1 ") {
            let fields = split_fields(rest, 7, "ordinary status")?;
            add_xy_entries(&mut entries, fields[0], fields[7], None)?;
            continue;
        }
        if let Some(rest) = record.strip_prefix("2 ") {
            let fields = split_fields(rest, 8, "rename or copy status")?;
            let original = records
                .next()
                .ok_or_else(|| harness_error("rename status is missing its original path"))?;
            let rename_from = (fields[7].starts_with('R')).then(|| original.to_owned());
            add_xy_entries(&mut entries, fields[0], fields[8], rename_from)?;
            continue;
        }
        if let Some(rest) = record.strip_prefix("u ") {
            let fields = split_fields(rest, 9, "unmerged status")?;
            entries.push(StatusEntry {
                group: "conflicted",
                path: fields[9].to_owned(),
                kind: "conflicted",
                rename_from: None,
            });
            continue;
        }
        if let Some(path) = record.strip_prefix("? ") {
            if !is_nested_git_dir(repo, path) {
                entries.push(StatusEntry {
                    group: "untracked",
                    path: path.to_owned(),
                    kind: "untracked",
                    rename_from: None,
                });
            }
            continue;
        }
        if record.starts_with("! ") {
            continue;
        }
        return Err(harness_error(format!(
            "unsupported porcelain v2 record: {record:?}"
        )));
    }
    canonical_entries(entries)
}

fn split_fields<'a>(
    record: &'a str,
    path_index: usize,
    kind: &str,
) -> Result<Vec<&'a str>, HarnessError> {
    let fields: Vec<_> = record.splitn(path_index + 1, ' ').collect();
    (fields.len() == path_index + 1)
        .then_some(fields)
        .ok_or_else(|| harness_error(format!("malformed {kind} porcelain record: {record:?}")))
}

fn add_xy_entries(
    entries: &mut Vec<StatusEntry>,
    xy: &str,
    path: &str,
    rename_from: Option<String>,
) -> Result<(), HarnessError> {
    let bytes = xy.as_bytes();
    if bytes.len() != 2 {
        return Err(harness_error(format!("invalid porcelain XY field: {xy:?}")));
    }
    if let Some((kind, from)) = status_kind(bytes[0], rename_from.clone())? {
        entries.push(StatusEntry {
            group: "staged",
            path: path.to_owned(),
            kind,
            rename_from: from,
        });
    }
    if let Some((kind, from)) = status_kind(bytes[1], None)? {
        entries.push(StatusEntry {
            group: "unstaged",
            path: path.to_owned(),
            kind,
            rename_from: from,
        });
    }
    Ok(())
}

fn status_kind(
    code: u8,
    rename_from: Option<String>,
) -> Result<Option<(&'static str, Option<String>)>, HarnessError> {
    match code {
        b'.' => Ok(None),
        b'A' => Ok(Some(("added", None))),
        b'M' => Ok(Some(("modified", None))),
        b'D' => Ok(Some(("deleted", None))),
        b'T' => Ok(Some(("type-change", None))),
        b'R' => Ok(Some(("renamed", rename_from))),
        b'C' => Ok(Some(("added", None))),
        _ => Err(harness_error(format!(
            "unsupported porcelain status code: {code}"
        ))),
    }
}

fn is_nested_git_dir(repo: &Path, path: &str) -> bool {
    repo.join(path).join(".git").exists()
}

fn canonical_entries(mut entries: Vec<StatusEntry>) -> Result<Value, HarnessError> {
    entries.sort_by(|left, right| {
        (
            left.group,
            left.path.as_str(),
            left.kind,
            left.rename_from.as_deref(),
        )
            .cmp(&(
                right.group,
                right.path.as_str(),
                right.kind,
                right.rename_from.as_deref(),
            ))
    });
    let mut keys = BTreeSet::new();
    for entry in &entries {
        let key = (
            entry.group,
            entry.path.as_str(),
            entry.kind,
            entry.rename_from.as_deref(),
        );
        if !keys.insert(key) {
            return Err(harness_error(format!(
                "duplicate canonical status entry: {} {} {}",
                entry.group, entry.path, entry.kind
            )));
        }
    }
    serde_json::to_value(entries).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_staged_unstaged_and_untracked_entries() {
        let root = tempfile::tempdir().unwrap();
        let value = parse_porcelain_v2(
            root.path(),
            "1 MM N... 100644 100644 100644 a b tracked.txt\0? new.txt\0",
        )
        .unwrap();

        assert_eq!(
            value,
            json!([
                {
                    "group": "staged",
                    "path": "tracked.txt",
                    "kind": "modified",
                    "rename_from": Value::Null,
                },
                {
                    "group": "unstaged",
                    "path": "tracked.txt",
                    "kind": "modified",
                    "rename_from": Value::Null,
                },
                {
                    "group": "untracked",
                    "path": "new.txt",
                    "kind": "untracked",
                    "rename_from": Value::Null,
                },
            ])
        );
    }

    #[test]
    fn preserves_rename_origin_from_the_nul_record() {
        let root = tempfile::tempdir().unwrap();
        let value = parse_porcelain_v2(
            root.path(),
            "2 R. N... 100644 100644 100644 a b R100 new.txt\0old.txt\0",
        )
        .unwrap();

        assert_eq!(
            value,
            json!([{
                "group": "staged",
                "path": "new.txt",
                "kind": "renamed",
                "rename_from": "old.txt",
            }])
        );
    }
}
