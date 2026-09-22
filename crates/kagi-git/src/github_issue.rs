//! Read-only Issue parsing for the `gh issue` JSON shapes.
//!
//! Split out of `github.rs` when that file passed the 800-LOC ceiling: pull
//! requests and Issues share the `gh` transport and two field helpers
//! ([`super::github::logins_at`], [`super::github::labels_at`]) but nothing
//! else, and the Issue half is self-contained. Pure; the callers are in
//! `github_fetch.rs`.

use kagi_domain::github::{Issue, IssueComment, IssueListSnapshot, IssueState};

use super::github::{labels_at, logins_at};
use crate::GitError;

/// One GraphQL request replaces `gh issue list` and adds exactly one search
/// alias for `mentions:@me`. Comment totals and the repository Issue nodes are
/// part of that same read, so rendering any tab never starts more I/O.
///
/// `$cursor` is nullable on purpose: `null` is the first page, and every later
/// page passes the previous response's `endCursor`. `pageInfo` travels with
/// the same page it describes, so the caller never has to guess whether a
/// short page was the last one (#752).
pub(crate) const ISSUE_LIST_QUERY: &str = r#"
query($owner: String!, $name: String!, $mentions: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    issues(first: 100, after: $cursor, states: OPEN, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number title state url createdAt updatedAt
        author { login }
        assignees(first: 20) { nodes { login } }
        labels(first: 20) { nodes { name color description } }
        comments { totalCount }
      }
    }
  }
  mentions: search(query: $mentions, type: ISSUE, first: 100) {
    nodes { ... on Issue { number } }
  }
}
"#;

/// The list metadata plus the conversation loaded for a selected issue.
pub(crate) const ISSUE_DETAIL_FIELDS: &str =
    "number,title,state,url,author,assignees,labels,body,comments,createdAt,updatedAt";

/// Parse the legacy flat Issue-list shape. Kept as a public pure parser for
/// callers and fixtures; the workspace transport now uses GraphQL below.
pub fn parse_issue_list(json: &str) -> Result<Vec<Issue>, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let Some(values) = value.as_array() else {
        return Err(GitError::Other("gh json: expected issue array".into()));
    };
    Ok(values.iter().filter_map(issue_from_value).collect())
}

/// Parse the atomic list + mention-membership response. `base_repo` is the
/// frozen identity used to address this very request and is carried
/// with the successful snapshot for later writes. `requested_cursor` is the
/// `after:` position this very response answered — `None` for the first page —
/// and is what proves the next cursor actually advanced.
pub fn parse_issue_list_snapshot(
    json: &str,
    base_repo: &str,
    requested_cursor: Option<&str>,
) -> Result<IssueListSnapshot, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    if let Some(errors) = value
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .filter(|errors| !errors.is_empty())
    {
        let detail = errors
            .iter()
            .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(GitError::Other(if detail.is_empty() {
            "gh graphql: partial response".into()
        } else {
            format!("gh graphql: {detail}")
        }));
    }
    let values = value
        .pointer("/data/repository/issues/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| GitError::Other("gh json: expected repository issue nodes".into()))?;
    let mentioned = value
        .pointer("/data/mentions/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| GitError::Other("gh json: expected mention search nodes".into()))?;
    let mut mentioned_numbers: Vec<u64> = mentioned
        .iter()
        .filter_map(|entry| entry.get("number").and_then(serde_json::Value::as_u64))
        .collect();
    mentioned_numbers.sort_unstable();
    mentioned_numbers.dedup();
    Ok(IssueListSnapshot {
        issues: values.iter().filter_map(issue_from_value).collect(),
        mentioned_numbers,
        base_repo: base_repo.to_string(),
        next_cursor: next_page_cursor(&value, requested_cursor)?,
    })
}

/// The cursor for the page *after* this one, or `None` when this was the last.
///
/// Every disagreement with the contract is an error, never an assumed end of
/// list: a missing or non-boolean `pageInfo.hasNextPage`, a `hasNextPage` with
/// no usable `endCursor`, and an `endCursor` equal to the cursor that was just
/// requested — that last one is a server answer that cannot advance, and
/// treating it as a page would append the same page forever.
fn next_page_cursor(
    value: &serde_json::Value,
    requested_cursor: Option<&str>,
) -> Result<Option<String>, GitError> {
    let page_info = value
        .pointer("/data/repository/issues/pageInfo")
        .ok_or_else(|| GitError::Other("gh json: expected issue pageInfo".into()))?;
    let has_next = page_info
        .get("hasNextPage")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| GitError::Other("gh json: expected pageInfo.hasNextPage".into()))?;
    if !has_next {
        return Ok(None);
    }
    let end_cursor = page_info
        .get("endCursor")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
        .ok_or_else(|| GitError::Other("gh json: hasNextPage without a usable endCursor".into()))?;
    if requested_cursor.is_some_and(|requested| requested == end_cursor) {
        return Err(GitError::Other(
            "gh json: pagination cursor did not advance".into(),
        ));
    }
    Ok(Some(end_cursor.to_string()))
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
    let assignees = connection_nodes(value, "assignees")
        .map(|nodes| {
            nodes
                .iter()
                .map(login)
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_else(|| logins_at(value, "assignees"));
    let labels = connection_nodes(value, "labels")
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?.to_string();
                    Some(kagi_domain::github::IssueLabel {
                        name,
                        color: entry
                            .get("color")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        description: entry
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_else(|| labels_at(value));
    let comments: Vec<IssueComment> = value
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
        comment_count: value
            .pointer("/comments/totalCount")
            .and_then(serde_json::Value::as_u64)
            .map(|count| usize::try_from(count).unwrap_or(usize::MAX))
            .unwrap_or(comments.len()),
        comments,
        created_at: string("createdAt"),
        updated_at: string("updatedAt"),
    })
}

fn connection_nodes<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Option<&'a Vec<serde_json::Value>> {
    value.get(key)?.get("nodes")?.as_array()
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
        assert_eq!(issue.comment_count, 1);
        assert_eq!(issue.comments[0].author, "bob");
        assert_eq!(issue.comments[0].updated_at, "");
    }

    #[test]
    fn parses_graphql_list_mentions_and_comment_totals_together() {
        let json = r#"{
          "data": {
            "repository": {"issues": {
              "pageInfo": {"hasNextPage": true, "endCursor": "Y3Vyc29yOjI="},
              "nodes": [{
              "number": 7, "title": "broken", "state": "OPEN",
              "url": "https://github.com/o/r/issues/7",
              "author": {"login": "alice"},
              "assignees": {"nodes": [{"login": "bob"}]},
              "labels": {"nodes": [{"name": "bug", "color": "d73a4a", "description": null}]},
              "comments": {"totalCount": 12},
              "createdAt": "t1", "updatedAt": "t2"
            }]}},
            "mentions": {"nodes": [{"number": 7}, {"number": 7}, {"number": 9}]}
          }
        }"#;
        let snapshot = parse_issue_list_snapshot(json, "github.com/o/r", None).unwrap();
        assert_eq!(snapshot.base_repo, "github.com/o/r");
        assert_eq!(snapshot.mentioned_numbers, vec![7, 9]);
        assert_eq!(snapshot.issues[0].assignees, vec!["bob"]);
        assert_eq!(snapshot.issues[0].labels[0].name, "bug");
        assert_eq!(snapshot.issues[0].comment_count, 12);
        assert!(snapshot.issues[0].comments.is_empty());
        assert_eq!(snapshot.next_cursor.as_deref(), Some("Y3Vyc29yOjI="));
    }

    /// The page boundary is only ever taken from the server's own answer.
    #[test]
    fn graphql_list_cursor_is_none_on_the_final_page() {
        let json = r#"{
          "data": {
            "repository": {"issues": {
              "pageInfo": {"hasNextPage": false, "endCursor": "Y3Vyc29yOjk="},
              "nodes": []
            }},
            "mentions": {"nodes": []}
          }
        }"#;
        let snapshot =
            parse_issue_list_snapshot(json, "github.com/o/r", Some("Y3Vyc29yOjg=")).unwrap();
        assert_eq!(
            snapshot.next_cursor, None,
            "a last page never hands back a cursor, even with an endCursor present"
        );
    }

    #[test]
    fn graphql_list_rejects_malformed_and_non_advancing_page_info() {
        let cases = [
            (
                r#"{"data":{"repository":{"issues":{"nodes":[]}},"mentions":{"nodes":[]}}}"#,
                "expected issue pageInfo",
                None,
            ),
            (
                r#"{"data":{"repository":{"issues":{"pageInfo":{"endCursor":"c2"},"nodes":[]}},
                   "mentions":{"nodes":[]}}}"#,
                "pageInfo.hasNextPage",
                None,
            ),
            (
                r#"{"data":{"repository":{"issues":{
                   "pageInfo":{"hasNextPage":true,"endCursor":null},"nodes":[]}},
                   "mentions":{"nodes":[]}}}"#,
                "usable endCursor",
                None,
            ),
            (
                r#"{"data":{"repository":{"issues":{
                   "pageInfo":{"hasNextPage":true,"endCursor":"  "},"nodes":[]}},
                   "mentions":{"nodes":[]}}}"#,
                "usable endCursor",
                None,
            ),
            (
                r#"{"data":{"repository":{"issues":{
                   "pageInfo":{"hasNextPage":true,"endCursor":"c2"},"nodes":[]}},
                   "mentions":{"nodes":[]}}}"#,
                "did not advance",
                Some("c2"),
            ),
        ];
        for (json, expected, requested) in cases {
            let error = parse_issue_list_snapshot(json, "github.com/o/r", requested)
                .expect_err("malformed pageInfo is never an implicit last page");
            assert!(
                error.to_string().contains(expected),
                "{error} does not mention {expected}"
            );
        }
    }

    #[test]
    fn graphql_list_rejects_missing_mentions_and_partial_errors() {
        let missing = r#"{"data":{"repository":{"issues":{"nodes":[]}}}}"#;
        assert!(parse_issue_list_snapshot(missing, "github.com/o/r", None)
            .unwrap_err()
            .to_string()
            .contains("mention search nodes"));

        let partial = r#"{
          "data":{"repository":{"issues":{"nodes":[]}},"mentions":null},
          "errors":[{"message":"search unavailable"}]
        }"#;
        assert!(parse_issue_list_snapshot(partial, "github.com/o/r", None)
            .unwrap_err()
            .to_string()
            .contains("search unavailable"));
    }

    #[test]
    fn issue_parsers_reject_wrong_top_level_shapes() {
        assert!(parse_issue_list("{}").is_err());
        assert!(parse_issue_detail("[]").is_err());
        assert!(parse_issue_detail(r#"{"title":"missing number"}"#).is_err());
    }
}
