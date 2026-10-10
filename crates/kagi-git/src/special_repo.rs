//! Read-only guards for repository semantics libgit2 cannot preserve.
use std::{collections::BTreeSet, path::Path};

use git2::{AttrCheckFlags, AttrValue, Repository, SubmoduleIgnore, SubmoduleStatus, Tree};
use kagi_domain::plan_note::{CommonNote, PlanNote, SparseCheckoutKind};

use crate::GitError;

/// Own a fresh attribute cache for each plan/preflight, including late changes
/// to .git/info/attributes. Never execute a configured clean/smudge command.
pub(crate) struct FilterCheck {
    repo: Repository,
    flags: AttrCheckFlags,
}

impl FilterCheck {
    pub(crate) fn new(repo: &Repository) -> Result<Self, GitError> {
        Ok(Self {
            repo: Repository::open(repo.path()).map_err(|e| {
                GitError::Other(format!(
                    "cannot read repository attributes: {}",
                    e.message()
                ))
            })?,
            flags: AttrCheckFlags::FILE_THEN_INDEX,
        })
    }

    /// Read committed .gitattributes without checking out the tree or changing
    /// the real index. Info/global attributes retain libgit2's usual precedence.
    fn for_tree(repo: &Repository, tree: &Tree<'_>) -> Result<Self, GitError> {
        let mut check = Self::new(repo)?;
        let mut index = git2::Index::new().map_err(git_error)?;
        index.read_tree(tree).map_err(git_error)?;
        check.repo.set_index(&mut index).map_err(git_error)?;
        check.flags = AttrCheckFlags::INDEX_ONLY;
        Ok(check)
    }

    pub(crate) fn blocker(&self, path: &Path) -> Result<Option<PlanNote>, GitError> {
        for attribute in ["filter", "diff", "merge"] {
            let value = self
                .repo
                .get_attr_bytes(path, attribute, self.flags)
                .map_err(|e| {
                    GitError::Other(format!(
                        "cannot read {attribute} attribute for '{}': {}",
                        path.display(),
                        e.message()
                    ))
                })?;
            // Built-in text/EOL conversions are not filter drivers. Kagi has
            // no supported named clean/smudge filter, including LFS. diff/merge
            // LFS markers also identify a pointer path when filter is unset.
            if let AttrValue::Bytes(driver) = AttrValue::always_bytes(value) {
                if !driver.is_empty() && (attribute == "filter" || driver == b"lfs") {
                    return Ok(Some(PlanNote::Common(CommonNote::ExternalFilter {
                        path: path.display().to_string(),
                        filter: String::from_utf8_lossy(driver).into_owned(),
                    })));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn require_supported(&self, path: &Path) -> Result<(), GitError> {
        match self.blocker(path)? {
            Some(note) => Err(GitError::Blocked(Box::new(note))),
            None => Ok(()),
        }
    }
}

fn git_error(error: git2::Error) -> GitError {
    GitError::Other(format!(
        "cannot inspect special-repository safety: {}",
        error.message()
    ))
}

fn config_bool(config: &git2::Config, key: &str) -> Result<Option<bool>, GitError> {
    match config.get_bool(key) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(git_error(error)),
    }
}

fn patterns_present(gitdir: &Path) -> Result<bool, GitError> {
    match std::fs::symlink_metadata(gitdir.join("info/sparse-checkout")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(GitError::Other(format!(
            "cannot inspect sparse-checkout patterns: {error}"
        ))),
    }
}

fn sparse_blocker(repo: &Repository) -> Result<Option<PlanNote>, GitError> {
    let config = repo.config().map_err(git_error)?;
    let enabled = config_bool(&config, "core.sparseCheckout")?;
    // Git ignores cone config and leftover patterns after sparse is disabled.
    // Patterns are worktree-local; the main worktree's common-dir patterns do
    // not describe a linked worktree.
    if enabled != Some(false) {
        let cone = config_bool(&config, "core.sparseCheckoutCone")?.unwrap_or(false);
        if enabled == Some(true) || cone || patterns_present(repo.path())? {
            let kind = if cone {
                SparseCheckoutKind::Cone
            } else {
                SparseCheckoutKind::NonCone
            };
            return Ok(Some(PlanNote::Common(
                CommonNote::SparseCheckoutUnsupported { kind },
            )));
        }
    }
    let skip_worktree = repo.index().map_err(git_error)?.iter().any(|entry| {
        git2::IndexEntryExtendedFlag::from_bits_truncate(entry.flags_extended)
            .contains(git2::IndexEntryExtendedFlag::SKIP_WORKTREE)
    });
    Ok(skip_worktree.then(|| {
        PlanNote::Common(CommonNote::SparseCheckoutUnsupported {
            kind: SparseCheckoutKind::SkipWorktree,
        })
    }))
}

fn submodule_blockers(repo: &Repository, blockers: &mut Vec<PlanNote>) -> Result<(), GitError> {
    for submodule in repo.submodules().map_err(git_error)? {
        // Never trust submodule.*.ignore: all nested work needs preservation.
        let status = repo
            .submodule_status(submodule.name().map_err(git_error)?, SubmoduleIgnore::None)
            .map_err(git_error)?;
        let uninitialized = status.contains(SubmoduleStatus::WD_UNINITIALIZED)
            || !status.contains(SubmoduleStatus::IN_WD)
            || submodule.open().is_err();
        let dirty = status.intersects(
            SubmoduleStatus::INDEX_ADDED
                | SubmoduleStatus::INDEX_DELETED
                | SubmoduleStatus::INDEX_MODIFIED
                | SubmoduleStatus::WD_ADDED
                | SubmoduleStatus::WD_DELETED
                | SubmoduleStatus::WD_MODIFIED
                | SubmoduleStatus::WD_INDEX_MODIFIED
                | SubmoduleStatus::WD_WD_MODIFIED
                | SubmoduleStatus::WD_UNTRACKED,
        );
        if uninitialized || dirty {
            blockers.push(PlanNote::Common(CommonNote::SubmoduleCheckoutUnsupported {
                path: submodule.path().display().to_string(),
                uninitialized,
            }));
        }
    }
    Ok(())
}

/// Read-only guard for a force checkout and the local work it must retain.
/// Call at both plan and preflight, before creating any backup or savepoint.
/// Ref-only operations must not call this: they do not replace this worktree.
pub(crate) fn force_checkout_blockers(
    repo: &Repository,
    target: &Tree<'_>,
    status: &crate::WorkingTreeStatus,
) -> Result<Vec<PlanNote>, GitError> {
    let current = FilterCheck::new(repo)?;
    let mut blockers = Vec::new();
    if let Some(note) = sparse_blocker(&current.repo)? {
        blockers.push(note);
    }
    submodule_blockers(&current.repo, &mut blockers)?;

    let head = repo
        .head()
        .and_then(|head| head.peel_to_tree())
        .map_err(git_error)?;
    let diff = repo
        .diff_tree_to_tree(Some(&head), Some(target), None)
        .map_err(git_error)?;
    let mut paths = BTreeSet::new();
    for delta in diff.deltas() {
        paths.extend(delta.old_file().path());
        paths.extend(delta.new_file().path());
    }
    for file in status.staged.iter().chain(&status.unstaged) {
        paths.insert(file.path.as_path());
        if let crate::ChangeKind::Renamed { from } = &file.change {
            paths.insert(from.as_path());
        }
    }
    paths.extend(
        status
            .untracked
            .iter()
            .chain(&status.conflicted)
            .map(|p| p.as_path()),
    );
    if paths.is_empty() {
        return Ok(blockers);
    }
    let original = FilterCheck::for_tree(repo, &head)?;
    let incoming = if head.id() == target.id() {
        None
    } else {
        Some(FilterCheck::for_tree(repo, target)?)
    };
    for path in paths {
        for check in [&current, &original].into_iter().chain(incoming.as_ref()) {
            if let Some(note) = check.blocker(path)? {
                blockers.push(match note {
                    PlanNote::Common(CommonNote::ExternalFilter { path, filter }) => {
                        PlanNote::Common(CommonNote::ExternalFilterSync { path, filter })
                    }
                    note => note,
                });
                break;
            }
        }
    }
    Ok(blockers)
}
