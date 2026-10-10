//! Exact-path, index-only unstage. Unlike reset_default, index APIs do not
//! interpret approved filenames as globs (#1130).
use crate::{resolve_head, GitError, Head};
use git2::{Index, IndexEntry, IndexTime, Repository};
use std::{collections::HashSet, path::Path};

fn other(error: git2::Error) -> GitError {
    GitError::Other(format!("unstage: {}", error.message()))
}

pub(crate) struct UnstagePlan<'a> {
    paths: Vec<&'a Path>,
    entries: Vec<Option<IndexEntry>>,
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
    let mut plan = UnstagePlan {
        paths: Vec::new(),
        entries: Vec::new(),
    };
    for path in paths {
        // Keep the existing explicit refusal for paths that cannot be represented
        // losslessly. POSIX backslashes remain filename bytes, never separators.
        let raw = crate::path_to_pathspec(path)?;
        #[cfg(windows)]
        let raw = raw.replace('\\', "/");
        let entry = match tree.as_ref().map(|tree| tree.get_path(path)) {
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
    }
    Ok(plan)
}

pub(crate) fn preflight_unstage<'a>(
    requested: impl Iterator<Item = &'a Path>,
    plan: &UnstagePlan<'_>,
) -> Result<(), GitError> {
    let requested: HashSet<_> = requested.collect();
    let approved: HashSet<_> = plan.paths.iter().copied().collect();
    if requested != approved {
        return Err(GitError::Other(
            "unstage refused: requested paths differ from approved paths".into(),
        ));
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
    for (path, entry) in plan.paths.iter().zip(&plan.entries) {
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
    index.write().map_err(other)?;
    verify_unstage(&mut index, plan)?;
    Ok(plan.paths.len())
}

fn verify_unstage(index: &mut Index, plan: &UnstagePlan<'_>) -> Result<(), GitError> {
    index.read(true).map_err(other)?;
    for (path, expected) in plan.paths.iter().zip(&plan.entries) {
        let actual = index.get_path(path, 0);
        if actual.as_ref().map(|entry| (entry.id, entry.mode))
            != expected.as_ref().map(|entry| (entry.id, entry.mode))
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
    #[test]
    fn unstage_literal_preflight_requires_exact_approved_set() {
        let a = Path::new("a[b].txt");
        let b = Path::new("ab.txt");
        let plan = UnstagePlan {
            paths: vec![a],
            entries: vec![None],
        };
        assert!(preflight_unstage([a].into_iter(), &plan).is_ok());
        for requested in [vec![], vec![b], vec![a, b]] {
            assert!(preflight_unstage(requested.into_iter(), &plan).is_err());
        }
    }
}
