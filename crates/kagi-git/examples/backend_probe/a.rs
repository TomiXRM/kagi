//! P4 read-path probes for #627.
//!
//! This module remains outside the root dispatcher until the integration owner
//! registers it. The implementation is nevertheless complete and compiled
//! through a temporary wrapper before measurements begin.

use std::collections::BTreeSet;
use std::path::Path;

use git2::Repository;
use kagi_git::{
    run_git, working_tree_status, ChangeKind, FileStatus, WorkingTreeStatus as DomainWorkingTreeStatus,
};
use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use serde::Serialize;
use serde_json::{json, Value};

/// A1: compare Kagi's libgit2 status read against parsed porcelain v2 output.
pub struct WorkingTreeStatus;

impl ProbeOperation for WorkingTreeStatus {
    fn name(&self) -> &'static str {
        "working-tree-status"
    }

    fn mutates_fixture(&self) -> bool {
        false
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

fn execute_libgit2(context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
    require_log_dir()?;
    let repo = Repository::open(context.repo())?;
    let status = working_tree_status(&repo).map_err(git_error)?;
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
    match candidate {
        "no-optional-locks" => args.push("--no-optional-locks"),
        "bare" => {}
        _ => {
            return Err(harness_error(format!(
                "working-tree-status has no CLI candidate {candidate:?}"
            )));
        }
    }
    args.extend([
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--renames",
    ]);
    let out = run_git(context.repo(), &args).map_err(git_error)?;
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
        ChangeKind::Renamed { from } => (
            "renamed",
            Some(from.to_string_lossy().replace('\\', "/")),
        ),
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

fn split_fields<'a>(record: &'a str, path_index: usize, kind: &str) -> Result<Vec<&'a str>, HarnessError> {
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

fn status_kind(code: u8, rename_from: Option<String>) -> Result<Option<(&'static str, Option<String>)>, HarnessError> {
    match code {
        b'.' => Ok(None),
        b'A' => Ok(Some(("added", None))),
        b'M' => Ok(Some(("modified", None))),
        b'D' => Ok(Some(("deleted", None))),
        b'T' => Ok(Some(("type-change", None))),
        b'R' => Ok(Some(("renamed", rename_from))),
        b'C' => Ok(Some(("added", None))),
        _ => Err(harness_error(format!("unsupported porcelain status code: {code}"))),
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
