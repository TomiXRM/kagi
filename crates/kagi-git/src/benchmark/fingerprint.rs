use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::HarnessError;

/// A stable logical role in the full repository fingerprint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FingerprintKind {
    Worktree,
    GitEntry,
    PrivateGitDir,
    CommonDir,
}

/// One recursively scanned root. `roles` has more than one value when a main
/// worktree's private gitdir and commondir resolve to the same canonical path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FingerprintRoot {
    /// Stable logical identifiers; never copy-specific filesystem paths.
    pub roles: Vec<FingerprintKind>,
    pub entries: Vec<FingerprintEntry>,
}

/// Metadata and content identity for one filesystem entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FingerprintEntry {
    pub relative_path: String,
    pub entry_type: String,
    pub size: u64,
    pub modified_ns: Option<String>,
    pub mode: u32,
    pub sha256: Option<String>,
    pub symlink_target: Option<String>,
}

/// Complete, deterministic snapshot used to reject a side-effecting prediction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Fingerprint {
    pub schema_version: u32,
    pub roots: Vec<FingerprintRoot>,
}

impl Fingerprint {
    pub fn changed_from(&self, before: &Self) -> bool {
        self != before
    }
}

/// Fingerprint the worktree and all Git storage that can change as a side
/// effect. The worktree's `.git` entry is recorded once as metadata, while its
/// contents are scanned through `repo.path()` to avoid a duplicate tree.
pub fn fingerprint_repository(repo_path: &Path) -> Result<Fingerprint, HarnessError> {
    let repo = git2::Repository::open(repo_path)?;
    let worktree = repo
        .workdir()
        .ok_or_else(|| HarnessError::new("bare repositories have no worktree fingerprint"))?
        .canonicalize()
        .map_err(|error| HarnessError::io(repo_path, error))?;

    let mut roots = vec![FingerprintRoot {
        roles: vec![FingerprintKind::Worktree],
        entries: scan_tree(&worktree, true)?,
    }];

    let git_entry = worktree.join(".git");
    if git_entry.exists() || fs::symlink_metadata(&git_entry).is_ok() {
        roots.push(FingerprintRoot {
            roles: vec![FingerprintKind::GitEntry],
            entries: vec![fingerprint_entry(&git_entry, Path::new("."))?],
        });
    }

    let mut git_roots: BTreeMap<PathBuf, Vec<FingerprintKind>> = BTreeMap::new();
    for (kind, path) in [
        (FingerprintKind::PrivateGitDir, repo.path()),
        (FingerprintKind::CommonDir, repo.commondir()),
    ] {
        let canonical = path
            .canonicalize()
            .map_err(|error| HarnessError::io(path, error))?;
        git_roots.entry(canonical).or_default().push(kind);
    }
    for (path, roles) in git_roots {
        roots.push(FingerprintRoot {
            roles,
            entries: scan_tree(&path, false)?,
        });
    }

    Ok(Fingerprint {
        schema_version: 1,
        roots,
    })
}

fn scan_tree(root: &Path, exclude_git_entry: bool) -> Result<Vec<FingerprintEntry>, HarnessError> {
    let mut entries = Vec::new();
    walk(root, root, exclude_git_entry, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

fn walk(
    root: &Path,
    path: &Path,
    exclude_git_entry: bool,
    entries: &mut Vec<FingerprintEntry>,
) -> Result<(), HarnessError> {
    for child in fs::read_dir(path).map_err(|error| HarnessError::io(path, error))? {
        let child = child.map_err(|error| HarnessError::io(path, error))?;
        let child_path = child.path();
        if exclude_git_entry && path == root && child.file_name() == ".git" {
            continue;
        }
        let relative = child_path
            .strip_prefix(root)
            .map_err(|_| HarnessError::new("fingerprint path escaped root"))?;
        let metadata = fs::symlink_metadata(&child_path)
            .map_err(|error| HarnessError::io(&child_path, error))?;
        entries.push(entry_from_metadata(&child_path, relative, &metadata)?);
        if metadata.is_dir() {
            walk(root, &child_path, false, entries)?;
        }
    }
    Ok(())
}

fn fingerprint_entry(path: &Path, relative: &Path) -> Result<FingerprintEntry, HarnessError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| HarnessError::io(path, error))?;
    entry_from_metadata(path, relative, &metadata)
}

fn entry_from_metadata(
    path: &Path,
    relative: &Path,
    metadata: &Metadata,
) -> Result<FingerprintEntry, HarnessError> {
    let file_type = metadata.file_type();
    let (entry_type, sha256, symlink_target) = if file_type.is_file() {
        ("file", Some(file_sha256(path)?), None)
    } else if file_type.is_dir() {
        ("directory", None, None)
    } else if file_type.is_symlink() {
        let target = fs::read_link(path).map_err(|error| HarnessError::io(path, error))?;
        ("symlink", None, Some(display_path(&target)))
    } else {
        ("other", None, None)
    };

    Ok(FingerprintEntry {
        relative_path: display_path(relative),
        entry_type: entry_type.to_owned(),
        size: metadata.len(),
        modified_ns: modified_ns(metadata),
        mode: mode(metadata),
        sha256,
        symlink_target,
    })
}

fn file_sha256(path: &Path) -> Result<String, HarnessError> {
    let mut file = File::open(path).map_err(|error| HarnessError::io(path, error))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| HarnessError::io(path, error))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn modified_ns(metadata: &Metadata) -> Option<String> {
    metadata
        .modified()
        .ok()
        .map(|time| match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => format!("+{}", duration.as_nanos()),
            Err(error) => format!("-{}", error.duration().as_nanos()),
        })
}

#[cfg(unix)]
fn mode(metadata: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn mode(metadata: &Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let status = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["init", "-q", "-b", "main"])
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(repo.join("tracked.txt"), b"same-size\n").unwrap();
        let status = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["add", "tracked.txt"])
            .status()
            .unwrap();
        assert!(status.success());
        root
    }

    #[test]
    fn detects_touch_same_size_rewrite_and_mode_change() {
        let root = fixture();
        let repo = root.path().join("repo");
        let file = repo.join("tracked.txt");

        let before_touch = fingerprint_repository(&repo).unwrap();
        let handle = File::options().write(true).open(&file).unwrap();
        handle
            .set_times(
                std::fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(2)),
            )
            .unwrap();
        assert!(fingerprint_repository(&repo)
            .unwrap()
            .changed_from(&before_touch));

        let before_rewrite = fingerprint_repository(&repo).unwrap();
        fs::write(&file, b"diff-size\n").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().len(), 10);
        assert!(fingerprint_repository(&repo)
            .unwrap()
            .changed_from(&before_rewrite));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let before_mode = fingerprint_repository(&repo).unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o100744)).unwrap();
            assert!(fingerprint_repository(&repo)
                .unwrap()
                .changed_from(&before_mode));
        }
    }

    #[test]
    fn linked_worktree_records_complete_logical_roots() {
        let root = fixture();
        let repo = root.path().join("repo");
        for args in [
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.test"],
            vec!["commit", "-q", "-m", "base"],
        ] {
            let status = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let linked = root.path().join("linked");
        let status = std::process::Command::new("git")
            .current_dir(&repo)
            .args([
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let fingerprint = fingerprint_repository(&linked).unwrap();
        let root_for = |kind| {
            fingerprint
                .roots
                .iter()
                .find(|root| root.roles.contains(&kind))
                .unwrap()
        };
        assert!(!root_for(FingerprintKind::Worktree).entries.is_empty());
        let git_entry = root_for(FingerprintKind::GitEntry);
        assert_eq!(git_entry.entries.len(), 1);
        assert_eq!(git_entry.entries[0].entry_type, "file");
        let private = root_for(FingerprintKind::PrivateGitDir);
        assert!(private
            .entries
            .iter()
            .any(|entry| entry.relative_path == "gitdir"));
        assert!(private
            .entries
            .iter()
            .any(|entry| entry.relative_path == "commondir"));
        let common = root_for(FingerprintKind::CommonDir);
        assert!(common
            .entries
            .iter()
            .any(|entry| entry.relative_path == "objects"));
        assert!(common
            .entries
            .iter()
            .any(|entry| entry.relative_path == "refs"));
    }
}
