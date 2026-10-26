//! Index identity and external-Git exclusion for destructive conflict routes.
use std::path::{Path, PathBuf};

use git2::{Index, Oid, Repository};

use crate::GitError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IndexIdentity(Vec<(Vec<u8>, u16, Oid, u32)>);

impl IndexIdentity {
    pub(super) fn capture(index: &Index, affected: impl Fn(&[u8]) -> bool) -> Self {
        Self(
            index
                .iter()
                .filter(|e| !affected(&e.path))
                .map(|e| (e.path, (e.flags >> 12) & 3, e.id, e.mode))
                .collect(),
        )
    }
}

/// The real Git lock excludes compliant external writers, not just Kagi tasks.
/// Keep it through verification. Serialize with public libgit2 APIs to a private
/// sibling index, then atomically replace the real index without releasing our
/// lock. Never remove a lock we did not create.
pub(super) struct ConflictIndexTransaction {
    lock_path: PathBuf,
    _lock: std::fs::File,
    index_path: PathBuf,
    temporary: tempfile::TempDir,
}

impl ConflictIndexTransaction {
    pub(super) fn acquire(repo: &Repository) -> Result<Self, GitError> {
        let index = repo.index().map_err(git_error)?;
        let index_path = index
            .path()
            .ok_or_else(|| GitError::Other("index has no disk path".into()))?
            .to_path_buf();
        let mut lock_name = index_path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let parent = index_path
            .parent()
            .ok_or_else(|| GitError::Other("index has no parent".into()))?;
        let temporary = tempfile::Builder::new()
            .prefix("kagi-conflict-index-")
            .tempdir_in(parent)
            .map_err(io_error)?;
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| GitError::Other(format!("{}: {error}", lock_path.display())))?;
        Ok(Self {
            lock_path,
            _lock: lock,
            index_path,
            temporary,
        })
    }

    pub(super) fn read(&self) -> Result<Index, GitError> {
        let private = self.temporary.path().join("index");
        std::fs::copy(&self.index_path, &private).map_err(io_error)?;
        Index::open(&private).map_err(git_error)
    }

    pub(super) fn write(&self, index: &mut Index) -> Result<(), GitError> {
        // An ownerless index skips libgit2's racy-clean smudge. Preserve that
        // protection before publishing a newer timestamp over the real index.
        let modified = filetime::FileTime::from_last_modification_time(
            &std::fs::metadata(&self.index_path).map_err(io_error)?,
        );
        let timestamp = (modified.unix_seconds(), modified.nanoseconds());
        for position in 0..index.len() {
            let mut entry = index.get(position).expect("entry within index length");
            if entry.flags & 0x3000 == 0
                && entry.mode != 0o160000
                && entry.file_size != 0
                && (i64::from(entry.mtime.seconds()), entry.mtime.nanoseconds()) >= timestamp
            {
                entry.file_size = 0;
                index.add(&entry).map_err(git_error)?;
            }
        }
        index.write().map_err(git_error)?;
        let private: &Path = index
            .path()
            .ok_or_else(|| GitError::Other("private index has no path".into()))?;
        std::fs::File::open(private)
            .and_then(|file| file.sync_all())
            .map_err(io_error)?;
        std::fs::rename(private, &self.index_path).map_err(io_error)
    }
}

impl Drop for ConflictIndexTransaction {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn git_error(error: git2::Error) -> GitError {
    GitError::Other(error.to_string())
}
fn io_error(error: std::io::Error) -> GitError {
    GitError::Other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{conflict_delete, dir_file_conflict};
    use kagi_domain::conflict_family::ConflictProgress;

    fn fixture(directory: bool) -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("thing"), b"file\n").unwrap();
        std::fs::write(tmp.path().join("unaffected"), b"original\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("thing")).unwrap();
        index.add_path(Path::new("unaffected")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("T", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
            .unwrap();
        drop(tree);
        let mut file = index.get_path(Path::new("thing"), 0).unwrap();
        index.remove(Path::new("thing"), 0).unwrap();
        file.flags |= (if directory { 2 } else { 1 }) << 12;
        if directory {
            std::fs::remove_file(tmp.path().join("thing")).unwrap();
            std::fs::create_dir(tmp.path().join("thing")).unwrap();
            std::fs::write(tmp.path().join("thing/child"), b"child\n").unwrap();
            index.add_path(Path::new("thing/child")).unwrap();
        }
        index.add(&file).unwrap();
        index.write().unwrap();
        (tmp, repo)
    }

    fn external_update(repo: &Repository) -> std::process::Output {
        let oid = repo.blob(b"external update\n").unwrap();
        std::process::Command::new("git")
            .args([
                "update-index",
                "--cacheinfo",
                &format!("100644,{oid},unaffected"),
            ])
            .current_dir(repo.workdir().unwrap())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", repo.workdir().unwrap())
            .output()
            .unwrap()
    }

    #[test]
    fn real_git_cannot_write_between_guarded_index_read_and_commit() {
        let (_tmp, repo) = fixture(false);
        let transaction = ConflictIndexTransaction::acquire(&repo).unwrap();
        let mut index = transaction.read().unwrap();
        let before = IndexIdentity::capture(&index, |_| false);
        let result = external_update(&repo);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("index.lock"));
        transaction.write(&mut index).unwrap();
        let mut actual = repo.index().unwrap();
        actual.read(true).unwrap();
        assert_eq!(IndexIdentity::capture(&actual, |_| false), before);
        // The real lock remains owned even after publication, through verification.
        assert!(!external_update(&repo).status.success());
        drop(transaction);
        assert!(external_update(&repo).status.success());
    }

    #[test]
    fn existing_git_lock_is_never_removed_or_overwritten() {
        let (_tmp, repo) = fixture(false);
        let lock = repo.path().join("index.lock");
        std::fs::write(&lock, b"another writer").unwrap();
        let error = ConflictIndexTransaction::acquire(&repo).err().unwrap();
        assert!(error.to_string().contains(&format!("{}: ", lock.display())));
        assert!(error.to_string().contains("File exists"));
        assert_eq!(std::fs::read(lock).unwrap(), b"another writer");
    }

    #[test]
    fn deletion_keeps_same_timestamp_same_size_unrelated_edit_dirty() {
        let (tmp, repo) = fixture(false);
        repo.config()
            .unwrap()
            .set_bool("core.trustctime", false)
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("core.checkstat", "minimal")
            .unwrap();
        let file = tmp.path().join("unaffected");
        let stamp = filetime::FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(&file, stamp).unwrap();
        let index_path = repo.path().join("index");
        // Seed a valid-looking stat cache without owner-aware auto-smudging.
        let mut index = Index::open(&index_path).unwrap();
        let mut entry = index.get_path(Path::new("unaffected"), 0).unwrap();
        let original_oid = entry.id;
        entry.mtime = git2::IndexTime::new(1_700_000_000, 0);
        entry.file_size = 9;
        index.add(&entry).unwrap();
        index.write().unwrap();
        drop(index);
        filetime::set_file_mtime(&index_path, stamp).unwrap();
        std::fs::write(&file, b"modified\n").unwrap();
        filetime::set_file_mtime(&file, stamp).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().len(), 9);

        let plan = conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
        conflict_delete::execute_conflict_delete(&repo, &plan, &mut None, &mut Vec::new(), |_| {
            Ok(())
        })
        .unwrap();
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        let entry = index.get_path(Path::new("unaffected"), 0).unwrap();
        assert_eq!(entry.id, original_oid);
        assert_eq!(entry.file_size, 0, "racy stat cache must be smudged");
        let diff = std::process::Command::new("git")
            .args(["diff-files", "--name-only", "--", "unaffected"])
            .current_dir(tmp.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", tmp.path())
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(diff.status.success(), "{:?}", diff.stderr);
        assert_eq!(diff.stdout, b"unaffected\n");
    }

    #[test]
    fn deletion_keeps_repository_admission_across_recovery_and_index_write() {
        let (_tmp, repo) = fixture(false);
        let plan = conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
        let mut boundaries = Vec::new();
        conflict_delete::execute_conflict_delete(
            &repo,
            &plan,
            &mut None,
            &mut Vec::new(),
            |progress| {
                let root = repo.workdir().unwrap().to_path_buf();
                let blocked = std::thread::spawn(move || {
                    let other = Repository::open(root).unwrap();
                    crate::write_lock::acquire(&other)
                        .err()
                        .is_some_and(|error| {
                            error.blocker()
                                == Some(&kagi_domain::plan_note::PlanNote::Conflicts(
                                    kagi_domain::plan_note::ConflictsNote::RepositoryBusy,
                                ))
                        })
                })
                .join()
                .unwrap();
                assert!(blocked, "repository admission must be held at {progress:?}");
                boundaries.push(progress);
                Ok(())
            },
        )
        .unwrap();
        assert!(boundaries.contains(&ConflictProgress::RecoveryCaptured));
        assert!(boundaries.contains(&ConflictProgress::WorktreeWritten));
        assert!(boundaries.contains(&ConflictProgress::IndexAndWorktreeWritten));
        let root = repo.workdir().unwrap().to_path_buf();
        assert!(std::thread::spawn(move || {
            crate::write_lock::acquire(&Repository::open(root).unwrap()).is_ok()
        })
        .join()
        .unwrap());
    }

    #[test]
    fn repeated_preflight_refuses_non_target_update_during_recovery() {
        use dir_file_conflict::DirFileChoice::{KeepDirectory, KeepFile};
        for choice in [None, Some(KeepFile), Some(KeepDirectory)] {
            let directory = choice.is_some();
            let (tmp, repo) = fixture(directory);
            let mut captured = None;
            let mut progress = |value| {
                if value == ConflictProgress::RecoveryCaptured {
                    assert!(external_update(&repo).status.success());
                    captured = Some(std::fs::read(repo.path().join("index")).unwrap());
                }
                Ok(())
            };
            let mut recovery = None;
            let result = if let Some(choice) = choice {
                let plan =
                    dir_file_conflict::plan_dir_file_resolution(&repo, Path::new("thing"), choice)
                        .unwrap();
                dir_file_conflict::apply_dir_file_resolution_with_progress(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            } else {
                let plan =
                    conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
                conflict_delete::execute_conflict_delete(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            };
            assert!(result.is_err());
            assert!(
                recovery.is_some(),
                "capture survives repeated-preflight failure"
            );
            assert_eq!(
                std::fs::read(repo.path().join("index")).unwrap(),
                captured.unwrap()
            );
            let path = if directory { "thing/child" } else { "thing" };
            assert!(tmp.path().join(path).exists());
            assert!(!repo.path().join("index.lock").exists());
        }
    }

    #[test]
    fn recovery_capture_callback_failure_retains_snapshot_before_mutation() {
        use dir_file_conflict::DirFileChoice::{KeepDirectory, KeepFile};
        for choice in [None, Some(KeepFile), Some(KeepDirectory)] {
            let (tmp, repo) = fixture(choice.is_some());
            let path = if choice.is_some() {
                "thing/child"
            } else {
                "thing"
            };
            let bytes = std::fs::read(tmp.path().join(path)).unwrap();
            let index = std::fs::read(repo.path().join("index")).unwrap();
            let mut recovery = None;
            let mut progress = |value| {
                assert_eq!(value, ConflictProgress::RecoveryCaptured);
                Err(GitError::Other(
                    "deterministic recovery callback stop".into(),
                ))
            };
            let result = if let Some(choice) = choice {
                let plan =
                    dir_file_conflict::plan_dir_file_resolution(&repo, Path::new("thing"), choice)
                        .unwrap();
                dir_file_conflict::apply_dir_file_resolution_with_progress(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            } else {
                let plan =
                    conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
                conflict_delete::execute_conflict_delete(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            };
            assert!(result.is_err());
            let snapshot = recovery.expect("capture must precede the fallible callback");
            let commit = repo
                .find_reference(&snapshot.refname)
                .unwrap()
                .peel_to_commit()
                .unwrap();
            assert_eq!(commit.id().to_string(), snapshot.commit);
            let tree = commit.tree().unwrap();
            let saved = tree.get_path(Path::new(path)).unwrap();
            assert_eq!(repo.find_blob(saved.id()).unwrap().content(), bytes);
            assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), bytes);
            assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
            assert!(!repo.path().join("index.lock").exists());
        }
    }

    #[test]
    fn destructive_routes_hold_git_lock_during_mutation_and_release_on_callback_error() {
        use dir_file_conflict::DirFileChoice::{KeepDirectory, KeepFile};
        for choice in [None, Some(KeepFile), Some(KeepDirectory)] {
            let directory = choice.is_some();
            let (tmp, repo) = fixture(directory);
            let mut reached = false;
            let mut progress = |value| {
                let boundary = if directory {
                    ConflictProgress::IndexWritten
                } else {
                    ConflictProgress::WorktreeWritten
                };
                if value == boundary {
                    reached = true;
                    assert!(!external_update(&repo).status.success());
                    return Err(GitError::Other("deterministic post-write stop".into()));
                }
                Ok(())
            };
            let mut recovery = None;
            let result = if let Some(choice) = choice {
                let plan =
                    dir_file_conflict::plan_dir_file_resolution(&repo, Path::new("thing"), choice)
                        .unwrap();
                dir_file_conflict::apply_dir_file_resolution_with_progress(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            } else {
                let plan =
                    conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
                conflict_delete::execute_conflict_delete(
                    &repo,
                    &plan,
                    &mut recovery,
                    &mut Vec::new(),
                    &mut progress,
                )
            };
            assert!(reached);
            assert!(result.is_err());
            assert!(
                recovery.is_some(),
                "capture survives post-write callback failure"
            );
            if !directory {
                assert!(!tmp.path().join("thing").exists());
            }
            assert!(!repo.path().join("index.lock").exists());
            assert!(external_update(&repo).status.success());
        }
    }

    #[test]
    fn verification_rejects_unaffected_update_after_resolution() {
        use dir_file_conflict::DirFileChoice::{KeepDirectory, KeepFile};
        for choice in [None, Some(KeepFile), Some(KeepDirectory)] {
            let (_tmp, repo) = fixture(choice.is_some());
            if let Some(choice) = choice {
                let plan =
                    dir_file_conflict::plan_dir_file_resolution(&repo, Path::new("thing"), choice)
                        .unwrap();
                dir_file_conflict::apply_dir_file_resolution_with_progress(
                    &repo,
                    &plan,
                    &mut None,
                    &mut Vec::new(),
                    |_| Ok(()),
                )
                .unwrap();
                assert!(external_update(&repo).status.success());
                assert!(dir_file_conflict::verify_dir_file_resolution(&repo, &plan).is_err());
            } else {
                let plan =
                    conflict_delete::plan_conflict_delete(&repo, Path::new("thing")).unwrap();
                conflict_delete::execute_conflict_delete(
                    &repo,
                    &plan,
                    &mut None,
                    &mut Vec::new(),
                    |_| Ok(()),
                )
                .unwrap();
                assert!(external_update(&repo).status.success());
                assert!(conflict_delete::verify_conflict_delete(&repo, &plan).is_err());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unaffected_identity_preserves_raw_paths_and_all_stages() {
        let (_tmp, repo) = fixture(false);
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(Path::new("unaffected"), 0).unwrap();
        entry.path = b"raw-\xff".to_vec();
        for stage in 1..=3 {
            entry.flags = (entry.flags & !0x3000) | (stage << 12);
            index.add(&entry).unwrap();
        }
        let captured = IndexIdentity::capture(&index, |p| p == b"thing");
        assert_eq!(
            captured
                .0
                .iter()
                .filter(|(p, _, _, _)| p == b"raw-\xff")
                .count(),
            3
        );
        entry.flags = (entry.flags & !0x3000) | (2 << 12);
        entry.id = repo.blob(b"changed stage two\n").unwrap();
        index.add(&entry).unwrap();
        assert_ne!(IndexIdentity::capture(&index, |p| p == b"thing"), captured);
    }
}
