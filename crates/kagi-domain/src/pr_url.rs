//! Pull/merge-request "create" URL construction (branch-menu "Create PR").
//!
//! Pure string parsing — no I/O, no git2. The UI resolves a remote URL (from
//! `Backend::remote_urls()`) and the two branch names, then calls
//! [`pr_create_url`] to build the link it hands to the platform's URL opener.

/// Recognized Git-hosting platforms, each with its own PR/MR "create" URL
/// shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Host {
    GitHub,
    GitLab,
    Bitbucket,
}

impl Host {
    fn domain(self) -> &'static str {
        match self {
            Host::GitHub => "github.com",
            Host::GitLab => "gitlab.com",
            Host::Bitbucket => "bitbucket.org",
        }
    }
}

const HOSTS: [Host; 3] = [Host::GitHub, Host::GitLab, Host::Bitbucket];

/// Parse `(owner, repo)` out of a git remote URL, given the literal `domain`
/// to look for. Handles the common forms:
///   * `https://<domain>/owner/repo.git`
///   * `https://<domain>/owner/repo`
///   * `git@<domain>:owner/repo.git`
///   * `ssh://git@<domain>/owner/repo.git`
fn owner_repo(remote_url: &str, domain: &str) -> Option<(String, String)> {
    let url = remote_url.trim();
    let scp_prefix = format!("git@{domain}:");

    let path = if let Some(rest) = url.strip_prefix(&scp_prefix) {
        rest
    } else {
        let idx = url.find(domain)?;
        let after = &url[idx + domain.len()..];
        after
            .strip_prefix('/')
            .or_else(|| after.strip_prefix(':'))?
    };

    let mut parts = path.trim_matches('/').splitn(2, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo_raw = parts.next().filter(|s| !s.is_empty())?;
    let repo = repo_raw
        .split('/')
        .next()
        .unwrap_or(repo_raw)
        .strip_suffix(".git")
        .unwrap_or_else(|| repo_raw.split('/').next().unwrap_or(repo_raw));

    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Detect which known host `remote_url` points at and extract `(owner, repo)`.
fn detect(remote_url: &str) -> Option<(Host, String, String)> {
    HOSTS.iter().find_map(|&host| {
        owner_repo(remote_url, host.domain()).map(|(owner, repo)| (host, owner, repo))
    })
}

/// Build the "create a pull/merge request" URL for `remote_url`, comparing
/// `head_branch` against `base_branch`. Returns `None` when `remote_url`
/// doesn't point at a recognized host (github.com / gitlab.com /
/// bitbucket.org) — the caller falls back to a plain-text notice.
pub fn pr_create_url(remote_url: &str, base_branch: &str, head_branch: &str) -> Option<String> {
    let (host, owner, repo) = detect(remote_url)?;
    Some(match host {
        Host::GitHub => format!(
            "https://github.com/{owner}/{repo}/compare/{base_branch}...{head_branch}?expand=1"
        ),
        Host::GitLab => format!(
            "https://gitlab.com/{owner}/{repo}/-/merge_requests/new?merge_request%5Bsource_branch%5D={head_branch}&merge_request%5Btarget_branch%5D={base_branch}"
        ),
        Host::Bitbucket => format!(
            "https://bitbucket.org/{owner}/{repo}/pull-requests/new?source={head_branch}&dest={base_branch}"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_https() {
        assert_eq!(
            pr_create_url("https://github.com/acme/widgets.git", "main", "feature/x"),
            Some("https://github.com/acme/widgets/compare/main...feature/x?expand=1".to_string())
        );
    }

    #[test]
    fn github_ssh() {
        assert_eq!(
            pr_create_url("git@github.com:acme/widgets.git", "main", "feature/x"),
            Some("https://github.com/acme/widgets/compare/main...feature/x?expand=1".to_string())
        );
    }

    #[test]
    fn gitlab_https() {
        assert_eq!(
            pr_create_url("https://gitlab.com/acme/widgets.git", "main", "feature/x"),
            Some(
                "https://gitlab.com/acme/widgets/-/merge_requests/new?merge_request%5Bsource_branch%5D=feature/x&merge_request%5Btarget_branch%5D=main"
                    .to_string()
            )
        );
    }

    #[test]
    fn bitbucket_ssh() {
        assert_eq!(
            pr_create_url("git@bitbucket.org:acme/widgets.git", "main", "feature/x"),
            Some(
                "https://bitbucket.org/acme/widgets/pull-requests/new?source=feature/x&dest=main"
                    .to_string()
            )
        );
    }

    #[test]
    fn unknown_host_is_none() {
        assert_eq!(
            pr_create_url("https://git.example.internal/acme/widgets.git", "main", "x"),
            None
        );
    }

    #[test]
    fn no_repo_path_is_none() {
        assert_eq!(pr_create_url("https://github.com/", "main", "x"), None);
    }
}
