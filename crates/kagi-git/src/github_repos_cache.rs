//! The last GitHub repository list Home read (#930): saved after every full
//! read and shown at once the next time Home opens, while a fresh read runs.
//!
//! A cache, not state: it is derived from `gh` and replaced wholesale by the
//! next read, so a file that does not parse is simply ignored and later
//! overwritten. The write is a temp file in the same folder renamed into
//! place, so a reader never sees half a file.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub use kagi_domain::github_repos::{OwnerRepos, RepoList, RepoListing};

/// The cache file next to `settings.json`.
pub fn cache_path(settings_dir: &Path) -> PathBuf {
    settings_dir.join("github_repos_cache.json")
}

/// Save `sections` to `path`. Best effort: an error is returned for the
/// caller to log, and the next read tries again.
pub fn save(path: &Path, sections: &[OwnerRepos]) -> std::io::Result<()> {
    let value = Value::Array(sections.iter().map(section_json).collect());
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&value)?)?;
    std::fs::rename(&tmp, path)
}

/// The saved list, or `None` when there is none or it does not parse.
pub fn load(path: &Path) -> Option<Vec<OwnerRepos>> {
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.as_array()?.iter().map(section_from).collect()
}

fn section_json(section: &OwnerRepos) -> Value {
    let list = match &section.list {
        Ok(list) => json!({
            "truncated": list.truncated,
            "repos": list.repos.iter().map(|r| json!({
                "nameWithOwner": r.name_with_owner,
                "host": r.host,
                "isFork": r.is_fork,
                "isPrivate": r.is_private,
                "description": r.description,
                "updatedAt": r.updated_at,
            })).collect::<Vec<_>>(),
        }),
        Err(error) => json!({ "error": error }),
    };
    json!({ "owner": section.owner, "list": list })
}

fn section_from(value: &Value) -> Option<OwnerRepos> {
    let owner = match value.get("owner")? {
        Value::Null => None,
        Value::String(owner) => Some(owner.clone()),
        _ => return None,
    };
    let list = value.get("list")?;
    let list = match list.get("error") {
        Some(error) => Err(error.as_str()?.to_string()),
        None => Ok(RepoList {
            truncated: list.get("truncated")?.as_bool()?,
            repos: list
                .get("repos")?
                .as_array()?
                .iter()
                .map(repo_from)
                .collect::<Option<_>>()?,
        }),
    };
    Some(OwnerRepos { owner, list })
}

fn repo_from(value: &Value) -> Option<RepoListing> {
    let text = |key: &str| value.get(key)?.as_str().map(str::to_string);
    Some(RepoListing {
        name_with_owner: text("nameWithOwner")?,
        host: text("host")?,
        is_fork: value.get("isFork")?.as_bool()?,
        is_private: value.get("isPrivate")?.as_bool()?,
        description: text("description")?,
        updated_at: text("updatedAt")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections() -> Vec<OwnerRepos> {
        vec![
            OwnerRepos {
                owner: None,
                list: Ok(RepoList {
                    truncated: true,
                    repos: vec![RepoListing {
                        name_with_owner: "acme/widgets".into(),
                        host: "ghe.example.com".into(),
                        is_fork: true,
                        is_private: false,
                        description: "d".into(),
                        updated_at: "2026-10-01T00:00:00Z".into(),
                    }],
                }),
            },
            OwnerRepos {
                owner: Some("locked-org".into()),
                list: Err("SAML".into()),
            },
        ]
    }

    /// What is saved is what is read back, unreadable owners and the
    /// truncation mark included.
    #[test]
    fn a_saved_list_reads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        save(&path, &sections()).unwrap();
        assert_eq!(load(&path), Some(sections()));
    }

    /// A damaged or foreign file is no cache: nothing is shown from it.
    #[test]
    fn a_file_that_does_not_parse_is_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        assert_eq!(load(&path), None);
        std::fs::write(&path, "{\"half").unwrap();
        assert_eq!(load(&path), None);
        let foreign = r#"[{"owner":1,"list":{"truncated":false,"repos":[]}}]"#;
        std::fs::write(&path, foreign).unwrap();
        assert_eq!(load(&path), None);
    }
}
