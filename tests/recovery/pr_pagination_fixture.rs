//! Bounded PR transport producers used by the native pagination consumers.
use crate::evidence_support::{pr_page, pull_request};
use crate::pr_fields_focus::OfflineGh;
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::PrListSnapshot;

pub(super) fn page(numbers: std::ops::Range<u64>, next: Option<&str>) -> PrListSnapshot {
    pr_page(
        numbers
            .map(|n| {
                let mut pr = pull_request(n, &format!("PR {n}"), "main");
                pr.updated_at = format!("{:020}", 1000 - n);
                pr
            })
            .collect(),
        "github.com/example/repo",
        next,
    )
}

pub(super) fn sidebar_refresh_gh() -> OfflineGh {
    first_page_gh("")
}

pub(super) fn periodic_refresh_gh(receipt: &std::path::Path) -> OfflineGh {
    let phase = receipt.with_extension("status-after");
    let mut status_cases = String::new();
    for pr in page(1..121, None).prs {
        let status = |after: bool| {
            serde_json::json!({
                "number": pr.number,
                "headRefOid": pr.head_sha,
                "mergeable": if after { "CONFLICTING" } else { "MERGEABLE" },
                "statusCheckRollup": [{
                    "__typename": "CheckRun",
                    "name": if after { "periodic-after" } else { "periodic-before" },
                    "workflowName": "fixture-ci",
                    "status": "COMPLETED",
                    "conclusion": if after { "FAILURE" } else { "SUCCESS" },
                    "detailsUrl": "https://github.com/example/repo/actions/runs/1",
                }],
            })
        };
        status_cases.push_str(&format!(
            "{} )\n\
             if [ -f '{}' ]; then\n\
               status_json='{}'\n\
             else\n\
               status_json='{}'\n\
             fi\n\
             printf '%s\\n' \"$status_json\"\n\
             exit 0 ;;\n",
            pr.number,
            phase.display(),
            status(true),
            status(false),
        ));
    }
    first_page_gh(&format!(
        "printf '%s\\n' \"$*\" >> '{}'\n\
         if [ \"$*\" = '--version' ]; then\n\
           printf 'gh version 2.0.0 (pagination fixture)\\n'\n\
           exit 0\n\
         fi\n\
         if [ \"$#\" -eq 7 ] && [ \"$1\" = 'pr' ] && [ \"$2\" = 'view' ] &&\n\
            [ \"$3\" = '-R' ] && [ \"$4\" = 'github.com/example/repo' ] &&\n\
            [ \"$6\" = '--json' ] &&\n\
            [ \"$7\" = 'number,headRefOid,statusCheckRollup,mergeable' ]; then\n\
           case \"$5\" in\n\
             {status_cases}\
           esac\n\
         fi\n",
        receipt.display(),
    ))
}

fn first_page_gh(prologue: &str) -> OfflineGh {
    let nodes: Vec<_> = page(1..101, Some("sidebar-page"))
        .prs
        .into_iter()
        .map(|pr| {
            serde_json::json!({
                "number": pr.number,
                "title": pr.title,
                "url": pr.url,
                "state": "OPEN",
                "isDraft": pr.is_draft,
                "isCrossRepository": pr.cross_repository,
                "createdAt": pr.created_at,
                "updatedAt": pr.updated_at,
                "headRefName": pr.head,
                "headRefOid": pr.head_sha,
                "baseRefName": pr.base,
                "reviewDecision": "APPROVED",
                "author": { "login": pr.author },
                "assignees": { "nodes": [] },
                "labels": { "nodes": [] },
                "reviewRequests": { "nodes": [] },
                "comments": { "totalCount": 0 },
            })
        })
        .collect();
    let identity = serde_json::json!({ "url": "https://github.com/example/repo" });
    let snapshot = serde_json::json!({
        "data": { "repository": { "pullRequests": {
            "nodes": nodes,
            "pageInfo": { "hasNextPage": true, "endCursor": "sidebar-page" },
        } } },
    });
    OfflineGh::with_script(&format!(
        r#"#!/bin/sh
{prologue}
case "$*" in
  "repo view --json url")
    printf '%s\n' '{identity}' ;;
  "api graphql --hostname github.com -F owner=example -F name=repo -F cursor=null -f states[]=OPEN -f query="*"pullRequests(first: 100,"*)
    printf '%s\n' '{snapshot}' ;;
  *)
    printf 'unsupported pagination fixture gh request: %s\n' "$*" >&2
    exit 1 ;;
esac
"#,
    ))
}

pub(super) fn ready(page: PrListSnapshot) {
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(page)));
}

pub(super) fn refresh(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    snapshot: PrListSnapshot,
) {
    ready(snapshot);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
}
