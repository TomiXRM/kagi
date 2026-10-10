//! Exact-path, index-only unstage. Unlike reset_default, index APIs do not
//! interpret approved filenames as globs (#1130).
use crate::{resolve_head, GitError, Head};
use git2::{Index, IndexEntry, IndexTime, Repository};
use kagi_domain::plan_note::{CommonNote, PlanNote};
use std::path::Path;

fn other(error: git2::Error) -> GitError {
    GitError::Other(format!("unstage: {}", error.message()))
}

pub(crate) struct UnstagePlan<'a> {
    paths: Vec<&'a Path>,
    entries: Vec<Option<IndexEntry>>,
    index_at_plan: Vec<TargetIndexState>,
}

#[derive(PartialEq, Eq)]
struct TargetIndexState {
    stage_zero: Option<(git2::Oid, u32)>,
    conflicted: bool,
}

fn target_index_state(index: &Index, path: &Path) -> TargetIndexState {
    TargetIndexState {
        stage_zero: index.get_path(path, 0).map(|entry| (entry.id, entry.mode)),
        conflicted: (1..=3).any(|stage| index.get_path(path, stage).is_some()),
    }
}

pub(crate) fn plan_unstage<'a>(
    repo: &Repository,
    paths: impl Iterator<Item = &'a Path>,
) -> Result<UnstagePlan<'a>, GitError> {
    let tree = match resolve_head(repo)? {
        Head::Unborn { .. } => None,
        _ => Some(
            repo.head()
                .and_then(|head| head.peel_to_tree())
                .map_err(other)?,
        ),
    };
    let mut index = repo.index().map_err(other)?;
    index.read(true).map_err(other)?;
    let mut plan = UnstagePlan {
        paths: Vec::new(),
        entries: Vec::new(),
        index_at_plan: Vec::new(),
    };
    for path in paths {
        // Keep the existing explicit refusal for paths that cannot be represented
        // losslessly. POSIX backslashes remain filename bytes, never separators.
        let raw = crate::path_to_pathspec(path)?;
        #[cfg(windows)]
        let raw = raw.replace('\\', "/");
        let entry = match tree.as_ref().map(|tree| tree.get_path(path)) {
            Some(Ok(entry)) if entry.kind() == Some(git2::ObjectType::Tree) => None,
            Some(Ok(entry)) => Some(IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: entry.filemode() as u32,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: entry.id(),
                flags: 0,
                flags_extended: 0,
                path: raw.as_bytes().to_vec(),
            }),
            Some(Err(error)) if error.code() != git2::ErrorCode::NotFound => {
                return Err(other(error))
            }
            _ => None,
        };
        plan.paths.push(path);
        plan.entries.push(entry);
        plan.index_at_plan.push(target_index_state(&index, path));
    }
    Ok(plan)
}

pub(crate) fn preflight_unstage(repo: &Repository, plan: &UnstagePlan<'_>) -> Result<(), GitError> {
    let mut index = repo.index().map_err(other)?;
    index.read(true).map_err(other)?;
    check_index_identity(&index, plan)
}

fn check_index_identity(index: &Index, plan: &UnstagePlan<'_>) -> Result<(), GitError> {
    for (path, expected) in plan.paths.iter().zip(&plan.index_at_plan) {
        if target_index_state(index, path) != *expected {
            return Err(GitError::Blocked(Box::new(PlanNote::Common(
                CommonNote::HunkChanged {
                    path: path.display().to_string(),
                },
            ))));
        }
    }
    Ok(())
}

pub(crate) fn execute_unstage(
    repo: &Repository,
    plan: &UnstagePlan<'_>,
) -> Result<usize, GitError> {
    if plan.paths.is_empty() {
        return Ok(0);
    }
    let mut index = repo.index().map_err(other)?;
    index.read(true).map_err(other)?;
    check_index_identity(&index, plan)?;
    let result = (|| {
        for (path, entry) in plan.paths.iter().zip(&plan.entries) {
            match index.conflict_remove(path) {
                Ok(()) => {}
                Err(error) if error.code() == git2::ErrorCode::NotFound => {}
                Err(error) => return Err(other(error)),
            }
            if let Some(entry) = entry {
                index.add(entry).map_err(other)?;
            } else {
                match index.remove_path(path) {
                    Ok(()) => {}
                    Err(error) if error.code() == git2::ErrorCode::NotFound => {}
                    Err(error) => return Err(other(error)),
                }
            }
        }
        index.write().map_err(other)
    })();
    if let Err(error) = result {
        // libgit2 shares this cached index with subsequent staging calls. Never
        // let a failed batch's edits leak into the next successful index write.
        let _ = index.read(true);
        return Err(error);
    }
    verify_unstage(&mut index, plan)?;
    Ok(plan.paths.len())
}

fn verify_unstage(index: &mut Index, plan: &UnstagePlan<'_>) -> Result<(), GitError> {
    index.read(true).map_err(other)?;
    for (path, expected) in plan.paths.iter().zip(&plan.entries) {
        let actual = index.get_path(path, 0);
        if actual.as_ref().map(|entry| (entry.id, entry.mode))
            != expected.as_ref().map(|entry| (entry.id, entry.mode))
            || (1..=3).any(|stage| index.get_path(path, stage).is_some())
        {
            return Err(GitError::Other(format!(
                "unstage verification failed: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        for path in ["a[b].txt", "b.txt", "other.txt"] {
            std::fs::write(tmp.path().join(path), "base\n").unwrap();
        }
        let mut index = repo.index().unwrap();
        for path in ["a[b].txt", "b.txt", "other.txt"] {
            index.add_path(Path::new(path)).unwrap();
        }
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
        drop(tree);
        for path in ["a[b].txt", "b.txt"] {
            std::fs::write(tmp.path().join(path), "staged\n").unwrap();
            index.add_path(Path::new(path)).unwrap();
        }
        index.write().unwrap();
        (tmp, repo)
    }

    #[test]
    fn unstage_literal_failed_batch_discards_cached_changes() {
        let (tmp, repo) = fixture();
        let mut plan = plan_unstage(
            &repo,
            [Path::new("a[b].txt"), Path::new("b.txt")].into_iter(),
        )
        .unwrap();
        // A real libgit2 add failure after the first path was restored.
        plan.entries[1].as_mut().unwrap().mode = 0o040000;
        let before = std::fs::read(repo.path().join("index")).unwrap();
        let selected = repo
            .index()
            .unwrap()
            .get_path(Path::new("a[b].txt"), 0)
            .unwrap()
            .id;
        assert!(execute_unstage(&repo, &plan).is_err());
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
        std::fs::write(tmp.path().join("other.txt"), "other staged\n").unwrap();
        crate::staging::stage_file(&repo, Path::new("other.txt")).unwrap();
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert_eq!(
            index.get_path(Path::new("a[b].txt"), 0).unwrap().id,
            selected
        );
    }

    #[test]
    fn unstage_literal_preflight_refuses_replaced_target_blob() {
        let (_tmp, repo) = fixture();
        let path = Path::new("a[b].txt");
        let plan = plan_unstage(&repo, std::iter::once(path)).unwrap();
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(path, 0).unwrap();
        entry.id = repo.blob(b"not approved\n").unwrap();
        index.add(&entry).unwrap();
        index.write().unwrap();
        let before = std::fs::read(repo.path().join("index")).unwrap();
        assert!(matches!(
            preflight_unstage(&repo, &plan),
            Err(GitError::Blocked(_))
        ));
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
    }
}
