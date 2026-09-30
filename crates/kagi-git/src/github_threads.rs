//! A pull request's review threads (#351, ADR-0209): one paginated
//! `gh api graphql` read of `PullRequestReviewThread`, which carries what the
//! REST `pulls/{n}/comments` list lacks — the anchor side, whether the thread
//! is outdated or resolved, and whether the viewer could resolve it. It
//! replaces that REST call, so opening a PR still makes the same number of
//! `gh` calls. The repository is the one `gh` resolves from the working
//! directory (`{owner}` / `{repo}` placeholders), as before.

use std::path::Path;

use kagi_domain::github::ReviewComment;
use kagi_domain::review_thread::{DiffSide, ReviewThread};

use crate::GitError;

const REVIEW_THREADS_QUERY: &str = "\
query($owner:String!,$name:String!,$number:Int!,$endCursor:String){\
 repository(owner:$owner,name:$name){\
  pullRequest(number:$number){\
   reviewThreads(first:100,after:$endCursor){\
    pageInfo{hasNextPage endCursor}\
    nodes{path line startLine originalLine diffSide isOutdated isResolved viewerCanResolve\
     comments(first:100){nodes{databaseId author{login} body createdAt diffHunk\
      replyTo{databaseId}}}}}}}}";

/// Every review thread of PR `number`, comments oldest first. A `gh` that
/// cannot answer (not installed, no GitHub remote, offline) reads as none,
/// like the REST list it replaces; a GraphQL error is reported.
pub fn pr_review_threads(workdir: &Path, number: u64) -> Result<Vec<ReviewThread>, GitError> {
    let out = crate::cli::gh_command()
        .args([
            "api",
            "graphql",
            "--paginate",
            "-F",
            "owner={owner}",
            "-F",
            "name={repo}",
            "-F",
            &format!("number={number}"),
            "-f",
            &format!("query={REVIEW_THREADS_QUERY}"),
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    parse_review_threads(&String::from_utf8_lossy(&out.stdout))
}

/// Parse the (possibly several, concatenated) pages `--paginate` prints.
pub fn parse_review_threads(pages: &str) -> Result<Vec<ReviewThread>, GitError> {
    let mut threads = Vec::new();
    for page in serde_json::Deserializer::from_str(pages).into_iter::<serde_json::Value>() {
        let page = page.map_err(|e| GitError::Other(format!("gh json: {e}")))?;
        if let Some(error) = crate::github::graphql_failure(&page) {
            return Err(error);
        }
        let nodes = page
            .pointer("/data/repository/pullRequest/reviewThreads/nodes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| GitError::Other("gh graphql: no reviewThreads".into()))?;
        threads.extend(nodes.iter().map(thread_from));
    }
    Ok(threads)
}

fn line_at(v: &serde_json::Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn thread_from(node: &serde_json::Value) -> ReviewThread {
    let flag = |key: &str| node.get(key).and_then(serde_json::Value::as_bool) == Some(true);
    let text = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let comments = node
        .pointer("/comments/nodes")
        .and_then(serde_json::Value::as_array)
        .map(|comments| {
            comments
                .iter()
                .map(|c| ReviewComment {
                    author: c
                        .pointer("/author/login")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    body: text(c, "body"),
                    diff_hunk: text(c, "diffHunk"),
                    created_at: text(c, "createdAt"),
                    in_reply_to: c
                        .pointer("/replyTo/databaseId")
                        .and_then(serde_json::Value::as_u64),
                    ..ReviewComment::default()
                })
                .collect()
        })
        .unwrap_or_default();
    ReviewThread {
        path: text(node, "path"),
        line: line_at(node, "line"),
        start_line: line_at(node, "startLine"),
        original_line: line_at(node, "originalLine"),
        diff_side: match node.get("diffSide").and_then(serde_json::Value::as_str) {
            Some("LEFT") => DiffSide::Left,
            _ => DiffSide::Right,
        },
        is_outdated: flag("isOutdated"),
        is_resolved: flag("isResolved"),
        viewer_can_resolve: flag("viewerCanResolve"),
        comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(nodes: &str, next: bool) -> String {
        format!(
            r#"{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{
              "pageInfo":{{"hasNextPage":{next},"endCursor":"c"}},"nodes":[{nodes}]}}}}}}}}}}"#
        )
    }

    const CURRENT: &str = r#"{"path":"a/b.py","line":872,"startLine":870,"originalLine":870,
      "diffSide":"RIGHT","isOutdated":false,"isResolved":false,"viewerCanResolve":true,
      "comments":{"nodes":[
        {"databaseId":9,"author":{"login":"Copilot"},"body":"fix\n```suggestion\nx = 1\n```",
         "createdAt":"t1","diffHunk":"@@ -1 +1 @@","replyTo":null},
        {"databaseId":10,"author":{"login":"me"},"body":"done","createdAt":"t2",
         "diffHunk":"","replyTo":{"databaseId":9}}]}}"#;
    const OUTDATED: &str = r#"{"path":"c.py","line":null,"startLine":null,"originalLine":12,
      "diffSide":"LEFT","isOutdated":true,"isResolved":true,"viewerCanResolve":false,
      "comments":{"nodes":[{"author":{"login":"x"},"body":"old","createdAt":"t3"}]}}"#;

    #[test]
    fn parses_threads_across_pages_with_side_state_and_replies() {
        let pages = format!("{}\n{}", page(CURRENT, true), page(OUTDATED, false));
        let threads = parse_review_threads(&pages).unwrap();
        assert_eq!(threads.len(), 2, "both --paginate pages are read");

        let current = &threads[0];
        assert_eq!(
            (
                current.path.as_str(),
                current.line,
                current.start_line,
                current.diff_side
            ),
            ("a/b.py", Some(872), Some(870), DiffSide::Right)
        );
        assert!(!current.is_outdated && current.viewer_can_resolve);
        assert_eq!(current.comments.len(), 2);
        assert_eq!(current.comments[0].author, "Copilot");
        assert!(current.comments[0].has_suggestion());
        assert_eq!(current.comments[1].in_reply_to, Some(9));

        let outdated = &threads[1];
        assert_eq!(outdated.diff_side, DiffSide::Left);
        assert_eq!((outdated.line, outdated.original_line), (None, Some(12)));
        assert!(outdated.is_outdated && outdated.is_resolved && !outdated.viewer_can_resolve);
        assert_eq!(outdated.anchor(), Some((12, true)));
    }

    #[test]
    fn a_graphql_error_is_a_failure_not_an_empty_list() {
        let err = parse_review_threads(r#"{"errors":[{"message":"no such PR"}]}"#).unwrap_err();
        assert!(err.to_string().contains("no such PR"), "{err}");
        assert!(parse_review_threads("not json").is_err());
    }
}
