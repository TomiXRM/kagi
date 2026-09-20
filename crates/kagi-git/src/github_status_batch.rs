//! Batched GraphQL transport for visible pull-request L2 status.

use std::collections::HashMap;
use std::path::Path;

use kagi_domain::github::{fold_ci, Check, Mergeable, PrStatusDetail};

use crate::github::PrFetchError;
use crate::GitError;

const MAX_BATCH: usize = 20;
const MIN_GATEWAY_BATCH: usize = 5;

#[derive(Debug, Clone)]
pub struct PrStatusBatchResult {
    pub number: u64,
    pub result: Result<PrStatusDetail, PrFetchError>,
}

pub fn pr_status_details_batch(
    workdir: &Path,
    base_repo: &str,
    numbers: &[u64],
) -> Vec<PrStatusBatchResult> {
    let Some((host, owner, name)) = split_repo_identity(base_repo) else {
        return failed(
            numbers,
            PrFetchError::Invalid(format!("invalid repository identity: {base_repo}")),
        );
    };
    let mut fetch = |chunk: &[u64]| fetch_chunk(workdir, host, owner, name, chunk);
    numbers
        .chunks(MAX_BATCH)
        .flat_map(|chunk| fetch_split(chunk, &mut fetch))
        .collect()
}

fn fetch_split(
    numbers: &[u64],
    fetch: &mut impl FnMut(&[u64]) -> Result<Vec<PrStatusDetail>, PrFetchError>,
) -> Vec<PrStatusBatchResult> {
    match fetch(numbers) {
        Ok(details) => results_for(numbers, details),
        Err(error) if is_batch_gateway_failure(&error) && numbers.len() > MIN_GATEWAY_BATCH => {
            let midpoint = numbers.len() / 2;
            let mut results = fetch_split(&numbers[..midpoint], fetch);
            results.extend(fetch_split(&numbers[midpoint..], fetch));
            results
        }
        Err(error) => failed(numbers, error),
    }
}

fn results_for(numbers: &[u64], details: Vec<PrStatusDetail>) -> Vec<PrStatusBatchResult> {
    let mut by_number: HashMap<u64, PrStatusDetail> = details
        .into_iter()
        .map(|detail| (detail.number, detail))
        .collect();
    numbers
        .iter()
        .map(|number| PrStatusBatchResult {
            number: *number,
            result: by_number.remove(number).ok_or_else(|| {
                PrFetchError::Invalid(format!("GraphQL response omitted pull request #{number}"))
            }),
        })
        .collect()
}

fn failed(numbers: &[u64], error: PrFetchError) -> Vec<PrStatusBatchResult> {
    numbers
        .iter()
        .map(|number| PrStatusBatchResult {
            number: *number,
            result: Err(error.clone()),
        })
        .collect()
}

fn is_batch_gateway_failure(error: &PrFetchError) -> bool {
    let detail = error.detail().to_ascii_lowercase();
    matches!(error, PrFetchError::Network(_) | PrFetchError::Unknown(_))
        && (detail.contains("http 502")
            || detail.contains("bad gateway")
            || detail.contains("http 504")
            || detail.contains("gateway timeout"))
}

fn fetch_chunk(
    workdir: &Path,
    host: &str,
    owner: &str,
    name: &str,
    numbers: &[u64],
) -> Result<Vec<PrStatusDetail>, PrFetchError> {
    let query = batch_query(numbers);
    let args = vec![
        "api".to_string(),
        "--hostname".to_string(),
        host.to_string(),
        "graphql".to_string(),
        "-f".to_string(),
        format!("query={query}"),
        "-F".to_string(),
        format!("owner={owner}"),
        "-F".to_string(),
        format!("name={name}"),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::github_fetch::fetch_json(workdir, &refs, |json| parse_pr_status_batch(json, numbers))
}

fn split_repo_identity(identity: &str) -> Option<(&str, &str, &str)> {
    let mut parts = identity.trim_end_matches('/').split('/');
    let result = (parts.next()?, parts.next()?, parts.next()?);
    parts.next().is_none().then_some(result)
}

fn batch_query(numbers: &[u64]) -> String {
    let mut query =
        String::from("query($owner:String!,$name:String!){repository(owner:$owner,name:$name){");
    for (index, number) in numbers.iter().enumerate() {
        query.push_str(&format!(
            "pr_{index}:pullRequest(number:{number}){{number headRefOid mergeable \
             commits(last:1){{nodes{{commit{{statusCheckRollup{{state contexts(first:50){{nodes{{\
             __typename ... on CheckRun{{name conclusion status detailsUrl workflowRun{{workflow{{name}}}}}} \
             ... on StatusContext{{context state targetUrl}}}}}}}}}}}}}}}}"
        ));
    }
    query.push_str("}}");
    query
}

pub fn parse_pr_status_batch(json: &str, numbers: &[u64]) -> Result<Vec<PrStatusDetail>, GitError> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|error| GitError::Other(format!("gh json: {error}")))?;
    let repository = root
        .pointer("/data/repository")
        .ok_or_else(|| GitError::Other("gh json: missing data.repository".into()))?;
    let mut details = Vec::new();
    for (index, expected) in numbers.iter().enumerate() {
        let Some(pr) = repository.get(format!("pr_{index}")) else {
            continue;
        };
        if pr.is_null() {
            continue;
        }
        details.push(status_detail(pr, *expected)?);
    }
    Ok(details)
}

fn status_detail(value: &serde_json::Value, expected: u64) -> Result<PrStatusDetail, GitError> {
    let number = value
        .get("number")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| GitError::Other("gh json: PR missing number".into()))?;
    if number != expected {
        return Err(GitError::Other(format!(
            "gh json: alias for #{expected} returned #{number}"
        )));
    }
    let nodes = value
        .pointer("/commits/nodes/0/commit/statusCheckRollup/contexts/nodes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let checks: Vec<Check> = nodes.iter().filter_map(check_from_graphql).collect();
    let conclusions: Vec<Option<&str>> = nodes.iter().map(check_conclusion).collect();
    Ok(PrStatusDetail {
        number,
        head_sha: text(value, "headRefOid"),
        ci: fold_ci(&conclusions),
        checks,
        mergeable: match text(value, "mergeable").as_str() {
            "MERGEABLE" => Mergeable::Clean,
            "CONFLICTING" => Mergeable::Conflicting,
            _ => Mergeable::Unknown,
        },
    })
}

fn check_from_graphql(value: &serde_json::Value) -> Option<Check> {
    match value
        .get("__typename")
        .and_then(serde_json::Value::as_str)?
    {
        "CheckRun" => Some(Check {
            name: text(value, "name"),
            workflow: value
                .pointer("/workflowRun/workflow/name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string(),
            state: fold_ci(&[check_conclusion(value)]),
            url: text(value, "detailsUrl"),
        }),
        "StatusContext" => Some(Check {
            name: text(value, "context"),
            workflow: String::new(),
            state: fold_ci(&[check_conclusion(value)]),
            url: text(value, "targetUrl"),
        }),
        _ => None,
    }
}

fn check_conclusion(value: &serde_json::Value) -> Option<&str> {
    value
        .get("conclusion")
        .or_else(|| value.get("state"))
        .and_then(serde_json::Value::as_str)
        .filter(|state| !state.is_empty())
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_pr_status_detail;
    use kagi_domain::github::CiState;

    const VIEW: &str = r#"{"number":42,"headRefOid":"abc","mergeable":"MERGEABLE","statusCheckRollup":[{"name":"build","workflowName":"ci","conclusion":"SUCCESS","detailsUrl":"https://check"},{"name":"legacy","workflowName":"","state":"FAILURE","targetUrl":"https://status"}]}"#;
    const GRAPHQL: &str = r#"{"data":{"repository":{"pr_0":{"number":42,"headRefOid":"abc","mergeable":"MERGEABLE","commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE","contexts":{"nodes":[{"__typename":"CheckRun","name":"build","conclusion":"SUCCESS","status":"COMPLETED","detailsUrl":"https://check","workflowRun":{"workflow":{"name":"ci"}}},{"__typename":"StatusContext","context":"legacy","state":"FAILURE","targetUrl":"https://status"}]}}}}]}}}}}"#;

    #[test]
    fn graphql_and_pr_view_status_have_the_same_domain_result() {
        let individual = parse_pr_status_detail(VIEW).unwrap();
        let batch = parse_pr_status_batch(GRAPHQL, &[42]).unwrap();
        assert_eq!(batch, vec![individual]);
    }

    #[test]
    fn gateway_failures_split_twenty_to_ten_to_five() {
        let numbers: Vec<u64> = (1..=20).collect();
        let mut sizes = Vec::new();
        let results = fetch_split(&numbers, &mut |chunk| {
            sizes.push(chunk.len());
            Err(PrFetchError::Unknown("HTTP 504 gateway timeout".into()))
        });
        assert_eq!(sizes, vec![20, 10, 5, 5, 10, 5, 5]);
        assert!(results.iter().all(|result| result.result.is_err()));
    }

    #[test]
    fn split_retry_keeps_partial_successes() {
        let numbers: Vec<u64> = (1..=20).collect();
        let results = fetch_split(&numbers, &mut |chunk| {
            if chunk.len() > 5 || chunk[0] > 10 {
                Err(PrFetchError::Unknown("HTTP 502 bad gateway".into()))
            } else {
                Ok(chunk
                    .iter()
                    .map(|number| PrStatusDetail {
                        number: *number,
                        head_sha: format!("head-{number}"),
                        ci: CiState::Success,
                        checks: Vec::new(),
                        mergeable: Mergeable::Clean,
                    })
                    .collect())
            }
        });
        assert!(results[..10].iter().all(|result| result.result.is_ok()));
        assert!(results[10..].iter().all(|result| result.result.is_err()));
    }

    #[test]
    fn query_uses_stable_aliases_and_never_exceeds_twenty_per_call() {
        let query = batch_query(&[7, 9]);
        assert!(query.contains("pr_0:pullRequest(number:7)"));
        assert!(query.contains("pr_1:pullRequest(number:9)"));
        assert_eq!(MAX_BATCH, 20);
    }
}
