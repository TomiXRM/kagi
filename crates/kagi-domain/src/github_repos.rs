//! The signed-in user's GitHub repositories, as the repository picker lists
//! them (#923), and their open pull requests and issues across every
//! repository (#928). Pure data; `kagi-git` reads it from `gh repo list` and
//! `gh search`.

/// One repository from `gh repo list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoListing {
    /// `owner/repo`, as GitHub spells it.
    pub name_with_owner: String,
    /// The server it lives on (`github.com`, or an Enterprise host).
    pub host: String,
    pub is_fork: bool,
    pub is_private: bool,
    pub description: String,
    /// RFC-3339, as `gh` returns it.
    pub updated_at: String,
}

impl RepoListing {
    /// `host/owner/repo` — what `gh repo clone` is given, so the clone comes
    /// from the same server the list did.
    pub fn clone_source(&self) -> String {
        format!("{}/{}", self.host, self.name_with_owner)
    }

    /// The lower-cased `host/owner/repo` a local clone's `origin` is
    /// compared against (GitHub names are case-insensitive).
    pub fn identity(&self) -> String {
        self.clone_source().to_ascii_lowercase()
    }

    /// The repository name: the default folder name of a clone.
    pub fn name(&self) -> &str {
        self.name_with_owner
            .rsplit('/')
            .next()
            .unwrap_or(&self.name_with_owner)
    }
}

/// A `gh repo list` read. `truncated`: the read hit its `--limit`, so more
/// repositories exist than are listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoList {
    pub repos: Vec<RepoListing>,
    pub truncated: bool,
}

/// One owner's section of the list: the signed-in user's own repositories
/// (`owner: None`) or one organization's. An organization that cannot be
/// read (SSO not authorized, …) keeps its section with the reason, so the
/// rest of the list still shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRepos {
    pub owner: Option<String>,
    pub list: Result<RepoList, String>,
}

/// One pull request or issue from `gh search prs` / `gh search issues`
/// (#928): enough to list it and to find its repository, locally or on the
/// server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// The server it lives on, from its URL (search names no host).
    pub host: String,
    /// `owner/repo`, as GitHub spells it.
    pub name_with_owner: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    /// The author's login.
    pub author: String,
    /// RFC-3339, as `gh` returns it.
    pub updated_at: String,
}

impl WorkItem {
    /// The lower-cased `host/owner/repo` of its repository — the key a local
    /// clone is matched by ([`RepoListing::identity`]).
    pub fn identity(&self) -> String {
        self.repo().identity()
    }

    /// Its repository as the clone card takes it. Search does not say
    /// whether the repository is a fork or private, so both read as no.
    pub fn repo(&self) -> RepoListing {
        RepoListing {
            name_with_owner: self.name_with_owner.clone(),
            host: self.host.clone(),
            is_fork: false,
            is_private: false,
            description: String::new(),
            updated_at: String::new(),
        }
    }

    /// Whether it lives on github.com: its author's login names a
    /// github.com user (and avatar) only then, not on an Enterprise host.
    pub fn on_github_com(&self) -> bool {
        self.host.eq_ignore_ascii_case("github.com")
    }
}

/// One `gh search` read. `truncated`: the read hit its `--limit`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkList {
    pub items: Vec<WorkItem>,
    pub truncated: bool,
}

/// Which `gh search` a [`WorkList`] answers (#928).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkKind {
    /// Open pull requests the user opened.
    MyPrs,
    /// Open pull requests waiting for the user's review.
    ReviewRequests,
    /// Open issues assigned to the user.
    AssignedIssues,
}

impl WorkKind {
    pub const ALL: [WorkKind; 3] = [
        WorkKind::MyPrs,
        WorkKind::ReviewRequests,
        WorkKind::AssignedIssues,
    ];

    pub fn is_pr(self) -> bool {
        !matches!(self, WorkKind::AssignedIssues)
    }
}

/// The three lists Home shows, one per [`WorkKind`]: a complete read, as it
/// is saved and read back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkLists {
    pub my_prs: WorkList,
    pub review_requests: WorkList,
    pub assigned_issues: WorkList,
}

impl WorkLists {
    pub fn get(&self, kind: WorkKind) -> &WorkList {
        match kind {
            WorkKind::MyPrs => &self.my_prs,
            WorkKind::ReviewRequests => &self.review_requests,
            WorkKind::AssignedIssues => &self.assigned_issues,
        }
    }

    pub fn get_mut(&mut self, kind: WorkKind) -> &mut WorkList {
        match kind {
            WorkKind::MyPrs => &mut self.my_prs,
            WorkKind::ReviewRequests => &mut self.review_requests,
            WorkKind::AssignedIssues => &mut self.assigned_issues,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_and_name() {
        let repo = RepoListing {
            name_with_owner: "Acme/Widgets".into(),
            host: "github.com".into(),
            is_fork: false,
            is_private: true,
            description: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(repo.clone_source(), "github.com/Acme/Widgets");
        assert_eq!(repo.identity(), "github.com/acme/widgets");
        assert_eq!(repo.name(), "Widgets");
    }
}
