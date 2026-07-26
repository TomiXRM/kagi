//! Inputs the commit-message author needs: the user's `commit.template` and a
//! list of people to offer as co-authors.
//!
//! Both are plain reads — nothing here mutates the repository, so there is no
//! `plan_/preflight_/execute_` triple. The pure text handling lives in
//! `kagi_domain::message`; this module only does the I/O.

use crate::GitError;
use git2::Repository;
use kagi_domain::message::strip_template_comments;
use std::path::{Path, PathBuf};

/// A person offered in the co-author picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorCandidate {
    pub name: String,
    pub email: String,
}

impl AuthorCandidate {
    /// The `Co-authored-by:` trailer line for this person.
    pub fn trailer(&self) -> String {
        format!("Co-authored-by: {} <{}>", self.name, self.email)
    }
}

/// Resolve `commit.template` and return its contents with comment lines
/// stripped, or `None` when unset / unreadable.
///
/// An unreadable template is deliberately `None` rather than an error: a stale
/// `commit.template` path is common and must not block committing.
pub fn load_commit_template(repo: &Repository) -> Option<String> {
    let cfg = repo.config().ok()?;
    let raw = cfg.get_string("commit.template").ok()?;
    let path = expand_tilde(raw.trim())?;
    // Relative paths resolve against the working directory, as git does.
    let path = if path.is_absolute() {
        path
    } else {
        repo.workdir()?.join(path)
    };
    let text = std::fs::read_to_string(path).ok()?;
    let stripped = strip_template_comments(&text);
    (!stripped.trim().is_empty()).then_some(stripped)
}

/// Expand a leading `~` against `$HOME`. Returns `None` for an empty path.
fn expand_tilde(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    match raw.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|h| Path::new(&h).join(rest)),
        None => Some(PathBuf::from(raw)),
    }
}

/// Distinct commit authors from recent history, most-recent first, excluding
/// the configured `user.email` (you are the author, not your own co-author).
///
/// `scan` bounds the walk; the picker only needs a handful of names and a full
/// walk on a large repo would stall the click.
pub fn recent_authors(
    repo: &Repository,
    scan: usize,
    limit: usize,
) -> Result<Vec<AuthorCandidate>, GitError> {
    let me = repo
        .config()
        .ok()
        .and_then(|c| c.get_string("user.email").ok())
        .unwrap_or_default()
        .to_lowercase();

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for commit in crate::commit_log(repo, scan)? {
        let email = commit.author.email.trim().to_string();
        if email.is_empty() || email.to_lowercase() == me {
            continue;
        }
        if !seen.insert(email.to_lowercase()) {
            continue;
        }
        out.push(AuthorCandidate {
            name: commit.author.name.trim().to_string(),
            email,
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_matches_the_git_convention() {
        let a = AuthorCandidate {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
        };
        assert_eq!(
            a.trailer(),
            "Co-authored-by: Ada Lovelace <ada@example.com>"
        );
    }

    #[test]
    fn expand_tilde_resolves_against_home() {
        std::env::set_var("HOME", "/home/ada");
        assert_eq!(
            expand_tilde("~/.gitmessage"),
            Some(PathBuf::from("/home/ada/.gitmessage"))
        );
        assert_eq!(expand_tilde("/etc/tpl"), Some(PathBuf::from("/etc/tpl")));
        assert_eq!(expand_tilde(""), None);
    }
}
