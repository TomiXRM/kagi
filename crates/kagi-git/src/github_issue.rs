//! Read-only Issue parsing for the `gh issue` JSON shapes.
//!
//! Split out of `github.rs` when that file passed the 800-LOC ceiling: pull
//! requests and Issues share the `gh` transport and two field helpers
//! ([`super::github::logins_at`], [`super::github::labels_at`]) but nothing
//! else, and the Issue half is self-contained. Pure; the callers are in
//! `github_fetch.rs`.

use kagi_domain::github::{Issue, IssueComment, IssueState};

use super::github::{labels_at, logins_at};
use crate::GitError;

/// Fields requested from `gh issue list`. `gh issue list` excludes pull
/// requests server-side; the parser also rejects PR-shaped values defensively.
pub(crate) const ISSUE_LIST_FIELDS: &str =
    "number,title,state,url,author,assignees,labels,createdAt,updatedAt";

/// The list metadata plus the conversation loaded for a selected issue.
pub(crate) const ISSUE_DETAIL_FIELDS: &str =
    "number,title,state,url,author,assignees,labels,body,comments,createdAt,updatedAt";

/// Parse `gh issue list --json <ISSUE_LIST_FIELDS>` output.
pub fn parse_issue_list(json: &str) -> Result<Vec<Issue>, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let Some(values) = value.as_array() else {
        return Err(GitError::Other("gh json: expected issue array".into()));
    };
    Ok(values.iter().filter_map(issue_from_value).collect())
}

/// Parse `gh issue view --json <ISSUE_DETAIL_FIELDS>` output.
pub fn parse_issue_detail(json: &str) -> Result<Issue, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    issue_from_value(&value).ok_or_else(|| GitError::Other("gh json: missing issue number".into()))
}

fn issue_from_value(value: &serde_json::Value) -> Option<Issue> {
    if value
        .get("isPullRequest")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || value.get("pullRequest").is_some_and(|v| !v.is_null())
    {
        return None;
    }
    let string = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    // One `{login}` object, not an array of them — `logins_at` answers the
    // array question, this one the scalar.
    let login = |entry: &serde_json::Value| {
        entry
            .get("login")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let state = IssueState::from_github(&string("state"));
    let assignees = logins_at(value, "assignees");
    let labels = labels_at(value);
    let comments = value
        .get("comments")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let body = entry
                        .get("body")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if body.trim().is_empty() {
                        return None;
                    }
                    Some(IssueComment {
                        author: entry.get("author").map(login).unwrap_or_default(),
                        body,
                        created_at: entry
                            .get("createdAt")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        updated_at: entry
                            .get("updatedAt")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Issue {
        number: value.get("number")?.as_u64()?,
        title: string("title"),
        state,
        url: string("url"),
        author: value.get("author").map(login).unwrap_or_default(),
        assignees,
        labels,
        body: string("body"),
        comments,
        created_at: string("createdAt"),
        updated_at: string("updatedAt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_issue_list_without_pull_requests_and_with_nullable_fields() {
        let json = r#"[
          {"number":7,"title":"real issue","state":"OPEN",
           "url":"https://github.com/o/r/issues/7","author":{"login":"alice"},
           "assignees":null,
           "labels":[{"name":"bug","color":"d73a4a","description":null}],
           "createdAt":"t1","updatedAt":"t2"},
          {"number":8,"title":"a pull request","state":"OPEN",
           "isPullRequest":true,"url":"https://github.com/o/r/pull/8"},
          {"number":9,"title":"sparse","state":"CLOSED","author":null}
        ]"#;
        let issues = parse_issue_list(json).unwrap();
        assert_eq!(
            issues.iter().map(|issue| issue.number).collect::<Vec<_>>(),
            vec![7, 9],
            "PR-shaped values never enter the issue list"
        );
        assert_eq!(issues[0].labels[0].description, "");
        assert!(issues[0].assignees.is_empty());
        assert_eq!(issues[1].state, IssueState::Closed);
        assert_eq!(issues[1].author, "");
    }

    #[test]
    fn parses_issue_detail_comments_and_missing_optional_fields() {
        let json = r#"{
          "number":7,"title":"broken","state":"OPEN","body":null,
          "comments":[
            {"author":{"login":"bob"},"body":"confirmed","createdAt":"t3","updatedAt":null},
            {"author":null,"body":"  "}
          ]
        }"#;
        let issue = parse_issue_detail(json).unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.body, "");
        assert_eq!(issue.comments.len(), 1);
        assert_eq!(issue.comments[0].author, "bob");
        assert_eq!(issue.comments[0].updated_at, "");
    }

    #[test]
    fn issue_parsers_reject_wrong_top_level_shapes() {
        assert!(parse_issue_list("{}").is_err());
        assert!(parse_issue_detail("[]").is_err());
        assert!(parse_issue_detail(r#"{"title":"missing number"}"#).is_err());
    }
}
