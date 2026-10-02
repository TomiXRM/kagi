//! The last GitHub repository list Home read (#930): saved after every full
//! read and shown at once the next time Home opens, while a fresh read runs.
//!
//! A cache, not state: it is derived from `gh` and replaced wholesale by the
//! next read, so a file that does not parse is simply ignored and later
//! overwritten. The write is a temp file in the same folder renamed into
//! place, so a reader never sees half a file.
//!
//! The list is bound to the account it was read as (`<host>/<login>`, from
//! [`crate::github_repos::active_account`]) and only read back for that
//! account: after `gh auth switch`, another account's private repository
//! names are not shown (#930 review).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub use kagi_domain::github_repos::{OwnerRepos, RepoList, RepoListing};

/// The cache file next to `settings.json`.
pub fn cache_path(settings_dir: &Path) -> PathBuf {
    settings_dir.join("github_repos_cache.json")
}

/// Save `sections`, read as `account`, to `path`. Best effort: an error is
/// returned for the caller to log, and the next read tries again.
///
/// On unix the file is readable by its owner only (`0600`), from its first
/// write on: it names private repositories. The temp file is created fresh
/// with that mode — `mode` applies only to a file being created, so one
/// left by an earlier interrupted save is removed first — and the rename
/// carries it over the old file.
pub fn save(path: &Path, account: &str, sections: &[OwnerRepos]) -> std::io::Result<()> {
    use std::io::Write as _;
    let value = json!({
        "account": account,
        "sections": sections.iter().map(section_json).collect::<Vec<_>>(),
    });
    let tmp = path.with_extension("json.tmp");
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(&serde_json::to_vec(&value)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

/// The list saved for `account`, or `None` when there is none, it was read
/// as another account, or it does not parse.
pub fn load(path: &Path, account: &str) -> Option<Vec<OwnerRepos>> {
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("account")?.as_str()? != account {
        return None;
    }
    value
        .get("sections")?
        .as_array()?
        .iter()
        .map(section_from)
        .collect()
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

    const ME: &str = "github.com/me";

    /// What is saved is what is read back, unreadable owners and the
    /// truncation mark included.
    #[test]
    fn a_saved_list_reads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        save(&path, ME, &sections()).unwrap();
        assert_eq!(load(&path, ME), Some(sections()));
    }

    /// A list read as one account is not read back for another: after
    /// `gh auth switch` its private repositories are not shown.
    #[test]
    fn a_list_is_read_back_only_for_its_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        save(&path, ME, &sections()).unwrap();
        assert_eq!(load(&path, "github.com/someone-else"), None);
        assert_eq!(load(&path, "ghe.example.com/me"), None);
    }

    /// The saved list names private repositories: owner-only from the
    /// first save, still owner-only after an update, and also when an
    /// earlier save left a world-readable temp file behind (#930 review).
    #[cfg(unix)]
    #[test]
    fn the_saved_list_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        save(&path, ME, &sections()).unwrap();
        assert_eq!(mode(&path), 0o600, "first save");
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, "left over").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();
        save(&path, ME, &sections()).unwrap();
        assert_eq!(mode(&path), 0o600, "update over a stale temp file");
        assert_eq!(load(&path, ME), Some(sections()));
    }

    /// A damaged or foreign file is no cache: nothing is shown from it.
    #[test]
    fn a_file_that_does_not_parse_is_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        assert_eq!(load(&path, ME), None);
        std::fs::write(&path, "{\"half").unwrap();
        assert_eq!(load(&path, ME), None);
        let foreign = format!(
            r#"{{"account":"{ME}","sections":[{{"owner":1,"list":{{"truncated":false,"repos":[]}}}}]}}"#
        );
        std::fs::write(&path, foreign).unwrap();
        assert_eq!(load(&path, ME), None);
    }
}
