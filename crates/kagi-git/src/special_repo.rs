//! Read-only guards for repository semantics libgit2 cannot preserve.
use std::path::Path;

use git2::{AttrCheckFlags, AttrValue, Repository};
use kagi_domain::plan_note::{CommonNote, PlanNote};

use crate::GitError;

/// Own a fresh attribute cache for each plan/preflight, including late changes
/// to .git/info/attributes. Never execute a configured clean/smudge command.
pub(crate) struct FilterCheck {
    repo: Repository,
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
        })
    }

    pub(crate) fn blocker(&self, path: &Path) -> Result<Option<PlanNote>, GitError> {
        for attribute in ["filter", "diff", "merge"] {
            let value = self
                .repo
                .get_attr_bytes(path, attribute, AttrCheckFlags::FILE_THEN_INDEX)
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
