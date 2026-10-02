//! The signed-in user's repositories and those of their organizations, from
//! `gh`, for Home (#923, #924). A read: it changes nothing.

use std::path::Path;

pub use kagi_domain::github_repos::{OwnerRepos, RepoList, RepoListing};

use crate::GitError;

/// How many repositories one read asks for. `gh repo list` returns 30 unless
/// told otherwise and says nothing when it stops; a list as long as this may
/// have been cut off, and [`RepoList::truncated`] says so.
pub const REPO_LIST_LIMIT: usize = 1000;

/// `gh repo list [<owner>] --limit <limit> --json …`: the user's own
/// repositories (`owner: None`) or one organization's, on `gh`'s default host.
pub fn repo_list_args(owner: Option<&str>, limit: usize) -> Vec<String> {
    let mut args = vec!["repo".to_string(), "list".to_string()];
    args.extend(owner.map(str::to_string));
    args.extend([
        "--limit".to_string(),
        limit.to_string(),
        "--json".to_string(),
        "nameWithOwner,url,isFork,isPrivate,description,updatedAt".to_string(),
    ]);
    args
}

/// `gh api user/orgs --paginate --jq .[].login`: the organizations the user
/// belongs to, one login per line.
pub fn org_list_args() -> Vec<String> {
    ["api", "user/orgs", "--paginate", "--jq", ".[].login"]
        .map(str::to_string)
        .to_vec()
}

/// Organization logins from [`org_list_args`]'s output. Pure. A line that is
/// not a plain login (letters, digits, `-`, not leading `-`) is dropped: it
/// is handed to `gh repo list` as an argument.
pub fn parse_org_logins(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|login| {
            !login.is_empty()
                && !login.starts_with('-')
                && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .map(str::to_string)
        .collect()
}

/// Read one owner's repositories. `workdir` is only the process's working
/// directory; `gh repo list` does not depend on it.
pub fn list_repos(workdir: &Path, owner: Option<&str>) -> Result<RepoList, GitError> {
    let stdout = crate::github_edit::read_gh(
        workdir,
        &repo_list_args(owner, REPO_LIST_LIMIT),
        "repo list",
    )?;
    parse_repo_list(&stdout, REPO_LIST_LIMIT)
}

/// The organizations the user belongs to. Empty when they cannot be read:
/// the user's own list still shows.
pub fn list_org_logins(workdir: &Path) -> Vec<String> {
    crate::github_edit::read_gh(workdir, &org_list_args(), "orgs")
        .map(|stdout| parse_org_logins(&stdout))
        .unwrap_or_default()
}

/// How many `gh repo list` reads run at once for the organizations.
const ORG_READS_AT_ONCE: usize = 6;

/// Each organization's repositories, one section per organization in the
/// order given, read a few at a time in parallel (one `gh` each). An
/// organization that cannot be read (SSO not authorized, …) keeps its
/// section with the reason.
pub fn list_org_repos(workdir: &Path, orgs: &[String]) -> Vec<OwnerRepos> {
    let mut sections = Vec::with_capacity(orgs.len());
    for chunk in orgs.chunks(ORG_READS_AT_ONCE) {
        std::thread::scope(|scope| {
            let reads: Vec<_> = chunk
                .iter()
                .map(|org| {
                    scope.spawn(move || {
                        list_repos(workdir, Some(org)).map_err(|error| error.to_string())
                    })
                })
                .collect();
            for (org, read) in chunk.iter().zip(reads) {
                let list = read
                    .join()
                    .unwrap_or_else(|_| Err("the read stopped unexpectedly".to_string()));
                sections.push(OwnerRepos {
                    owner: Some(org.clone()),
                    list,
                });
            }
        });
    }
    sections
}

/// The `host/owner/repo` identity (lower-cased, as
/// [`RepoListing::identity`]) of the repository at `path`'s `origin`, so a
/// listed repository can be matched with a local clone. `None` when the path
/// is not a repository, has no `origin`, or its URL names no repository.
pub fn origin_identity(path: &Path) -> Option<String> {
    let repo = git2::Repository::open(path).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    crate::backend::remote_ref::repo_identity(remote.url().ok()?)
}

/// Parse `gh repo list --json …`. Pure; unit-tested. An entry without a name
/// or a URL whose host can be read is skipped — it could not be cloned.
/// `truncated` counts what `gh` returned, skipped entries included.
pub fn parse_repo_list(json: &str, limit: usize) -> Result<RepoList, GitError> {
    let value: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| GitError::Other(format!("gh repo list json: {e}")))?;
    let entries = value
        .as_array()
        .ok_or_else(|| GitError::Other("gh repo list: expected a JSON array".to_string()))?;
    let text = |entry: &serde_json::Value, key: &str| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let flag = |entry: &serde_json::Value, key: &str| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let repos = entries
        .iter()
        .filter_map(|entry| {
            let name_with_owner = text(entry, "nameWithOwner");
            let host = url_host(&text(entry, "url"))?;
            (name_with_owner.split('/').count() == 2).then(|| RepoListing {
                name_with_owner,
                host,
                is_fork: flag(entry, "isFork"),
                is_private: flag(entry, "isPrivate"),
                description: text(entry, "description"),
                updated_at: text(entry, "updatedAt"),
            })
        })
        .collect();
    Ok(RepoList {
        repos,
        truncated: entries.len() >= limit,
    })
}

/// The host of an `https://host/owner/repo` URL.
fn url_host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_names_its_owner_and_limit() {
        assert_eq!(
            &repo_list_args(None, 1000)[..4],
            ["repo", "list", "--limit", "1000"]
        );
        assert_eq!(
            &repo_list_args(Some("acme-org"), 1000)[..5],
            ["repo", "list", "acme-org", "--limit", "1000"]
        );
    }

    /// Each login becomes a `gh repo list` argument: anything that is not a
    /// plain login (a flag, a path, blank) is dropped.
    #[test]
    fn org_logins_are_plain_names_only() {
        assert_eq!(
            parse_org_logins("acme-org\n\n  Other1 \n--upload-pack=x\nbad/path\n"),
            ["acme-org", "Other1"]
        );
    }

    #[test]
    fn parses_repositories_and_skips_unclonable_entries() {
        let json = r#"[
          {"nameWithOwner":"acme/widgets","url":"https://github.com/acme/widgets",
           "isFork":true,"isPrivate":false,"description":"d","updatedAt":"2026-10-01T00:00:00Z"},
          {"nameWithOwner":"acme/ghe","url":"https://ghe.example.com/acme/ghe"},
          {"nameWithOwner":"","url":"https://github.com/x/y"},
          {"nameWithOwner":"acme/nourl"}
        ]"#;
        let list = parse_repo_list(json, 1000).unwrap();
        assert!(!list.truncated);
        assert_eq!(list.repos.len(), 2);
        assert_eq!(list.repos[0].clone_source(), "github.com/acme/widgets");
        assert!(list.repos[0].is_fork);
        assert_eq!(list.repos[0].updated_at, "2026-10-01T00:00:00Z");
        assert_eq!(list.repos[1].clone_source(), "ghe.example.com/acme/ghe");
        assert!(!list.repos[1].is_private);
    }

    /// As many entries as the limit: more may exist, and the picker says so.
    #[test]
    fn a_full_page_is_truncated() {
        let json = r#"[{"nameWithOwner":"a/b","url":"https://github.com/a/b"},
                       {"nameWithOwner":"a/c","url":"https://github.com/a/c"}]"#;
        assert!(parse_repo_list(json, 2).unwrap().truncated);
        assert!(!parse_repo_list(json, 3).unwrap().truncated);
    }

    #[test]
    fn malformed_output_is_an_error_not_an_empty_list() {
        assert!(parse_repo_list("not json", 10).is_err());
        assert!(parse_repo_list(r#"{"a":1}"#, 10).is_err());
    }
}
