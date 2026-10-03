//! The signed-in user's open pull requests and issues across every
//! repository, from `gh search` (#928): the PRs they opened, the PRs waiting
//! for their review, and the issues assigned to them. A read: it changes
//! nothing.

use std::path::Path;

pub use kagi_domain::github_repos::{WorkItem, WorkKind, WorkList};

use crate::GitError;

/// How many results one search asks for. `gh search` stops at its limit and
/// says nothing; a list as long as this may have been cut off, and
/// [`WorkList::truncated`] says so.
pub const WORK_LIST_LIMIT: usize = 100;

const PR_FIELDS: &str = "number,title,url,repository,isDraft,author,updatedAt";
const ISSUE_FIELDS: &str = "number,title,url,repository,author,updatedAt";

/// `gh search … --state=open --limit <limit> --json …` for `kind`, on `gh`'s
/// default host.
pub fn search_args(kind: WorkKind, limit: usize) -> Vec<String> {
    let (what, filter, fields) = match kind {
        WorkKind::MyPrs => ("prs", "--author=@me", PR_FIELDS),
        WorkKind::ReviewRequests => ("prs", "--review-requested=@me", PR_FIELDS),
        WorkKind::AssignedIssues => ("issues", "--assignee=@me", ISSUE_FIELDS),
    };
    [
        "search",
        what,
        filter,
        "--state=open",
        "--limit",
        &limit.to_string(),
        "--json",
        fields,
    ]
    .map(str::to_string)
    .to_vec()
}

/// Read one of the user's lists. `workdir` is only the process's working
/// directory; `gh search` does not depend on it.
pub fn search_work(workdir: &Path, kind: WorkKind) -> Result<WorkList, GitError> {
    let stdout =
        crate::github_edit::read_gh(workdir, &search_args(kind, WORK_LIST_LIMIT), "search")?;
    parse_work_list(&stdout, WORK_LIST_LIMIT)
}

/// Parse `gh search prs|issues --json …`. Pure; unit-tested. An entry
/// without a number, an `owner/repo` or a URL whose host can be read is
/// skipped: it could not be opened. `truncated` counts what `gh` returned,
/// skipped entries included.
pub fn parse_work_list(json: &str, limit: usize) -> Result<WorkList, GitError> {
    let value: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| GitError::Other(format!("gh search json: {e}")))?;
    let entries = value
        .as_array()
        .ok_or_else(|| GitError::Other("gh search: expected a JSON array".to_string()))?;
    let items = entries.iter().filter_map(work_item).collect();
    Ok(WorkList {
        items,
        truncated: entries.len() >= limit,
    })
}

fn work_item(entry: &serde_json::Value) -> Option<WorkItem> {
    let text = |pointer: &str| {
        entry
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let name_with_owner = text("/repository/nameWithOwner");
    let url = text("/url");
    let host = crate::github_repos::url_host(&url)?;
    if name_with_owner.split('/').count() != 2 {
        return None;
    }
    Some(WorkItem {
        host,
        name_with_owner,
        number: entry.get("number")?.as_u64()?,
        title: text("/title"),
        url,
        is_draft: entry
            .get("isDraft")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        author: text("/author/login"),
        updated_at: text("/updatedAt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository is found from `nameWithOwner` and the URL's host —
    /// an Enterprise host included — and an unopenable entry is skipped.
    #[test]
    fn parses_items_and_skips_unopenable_ones() {
        let json = r#"[
          {"number":7,"title":"Fix","url":"https://ghe.example.com/Acme/Widgets/pull/7",
           "repository":{"name":"Widgets","nameWithOwner":"Acme/Widgets"},
           "isDraft":true,"author":{"login":"octo"},"updatedAt":"2026-10-01T00:00:00Z"},
          {"number":8,"title":"No repo","url":"https://github.com/a/b/issues/8","repository":{}},
          {"title":"No number","url":"https://github.com/a/b/issues/9",
           "repository":{"nameWithOwner":"a/b"}}
        ]"#;
        let list = parse_work_list(json, 100).unwrap();
        assert!(!list.truncated);
        assert_eq!(list.items.len(), 1);
        let item = &list.items[0];
        assert_eq!(item.identity(), "ghe.example.com/acme/widgets");
        assert_eq!(item.repo().clone_source(), "ghe.example.com/Acme/Widgets");
        assert_eq!((item.number, item.is_draft), (7, true));
        assert_eq!(item.author, "octo");
    }

    /// As many results as the limit: more may exist, and Home says so.
    #[test]
    fn a_full_page_is_truncated() {
        let json = r#"[{"number":1,"url":"https://github.com/a/b/pull/1",
                        "repository":{"nameWithOwner":"a/b"}}]"#;
        assert!(parse_work_list(json, 1).unwrap().truncated);
        assert!(!parse_work_list(json, 2).unwrap().truncated);
    }

    #[test]
    fn malformed_output_is_an_error_not_an_empty_list() {
        assert!(parse_work_list("not json", 10).is_err());
        assert!(parse_work_list(r#"{"a":1}"#, 10).is_err());
    }
}
