//! The signed-in user's GitHub repositories, as the repository picker lists
//! them (#923). Pure data; `kagi-git` reads it from `gh repo list`.

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
