"""Release evidence comes from ci.yml's aggregate, never commit check-name matching."""

from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass
from typing import Any, Literal
from urllib.parse import urlencode

WORKFLOW_PATH = ".github/workflows/ci.yml"
# This is the aggregate job ID (it deliberately has no separate display name).
BLOCKING_JOB = "blocking-ci"
PENDING = frozenset({"queued", "in_progress", "waiting", "requested", "pending"})


@dataclass(frozen=True)
class Decision:
    state: Literal["success", "wait", "failure"]
    message: str


def needs_decision(needs: object) -> Decision:
    """GitHub supplies every declared `needs` entry, including the whole matrix result."""
    if not isinstance(needs, dict) or not needs:
        return Decision("failure", "blocking CI has no dependency results")
    for name, dependency in needs.items():
        result = dependency.get("result") if isinstance(dependency, dict) else None
        if result != "success":
            return Decision("failure", f"blocking dependency {name}: {result or 'missing result'}")
    return Decision("success", "every blocking CI dependency succeeded")


def latest_run(runs: list[dict[str, Any]], sha: str, workflow_id: int) -> dict[str, Any] | None:
    """Newest run number wins; rerunning an older run never supersedes a newer run."""
    matching = [
        run for run in runs if run.get("head_sha") == sha and run.get("workflow_id") == workflow_id
    ]
    return max(matching, key=lambda run: (run["run_number"], run["id"]), default=None)


def run_decision(
    run: dict[str, Any], jobs: list[dict[str, Any]], sha: str, workflow_id: int
) -> Decision:
    if run.get("head_sha") != sha or run.get("workflow_id") != workflow_id:
        return Decision("failure", "CI run does not belong to the target SHA and workflow")
    run_id, attempt = run["id"], run["run_attempt"]
    label = f"{WORKFLOW_PATH} run {run_id} attempt {attempt} on {sha}"
    matching = [
        job
        for job in jobs
        if job.get("name") == BLOCKING_JOB
        and job.get("run_id") == run_id
        and job.get("run_attempt") == attempt
        and job.get("head_sha") == sha
    ]
    if not matching:
        if run.get("status") in PENDING:
            return Decision("wait", f"{label}: aggregate not reported yet")
        return Decision("failure", f"{label}: completed without an aggregate for this attempt")
    if len(matching) != 1:
        return Decision("failure", f"{label}: ambiguous aggregate jobs")
    job = matching[0]
    if job.get("status") == "completed":
        if job.get("conclusion") == "success":
            # Advisory jobs need not finish or succeed. Only the aggregate owns
            # the required set, not the workflow's overall conclusion.
            return Decision("success", f"{label}: blocking CI succeeded")
        return Decision("failure", f"{label}: aggregate concluded {job.get('conclusion')}")
    if job.get("status") in PENDING and run.get("status") in PENDING:
        return Decision("wait", f"{label}: aggregate is {job['status']}")
    return Decision("failure", f"{label}: aggregate has no successful completion")


def _gh_api(path: str, *, paginate: bool = False) -> Any:
    command = ["gh", "api", "--method", "GET", path]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    response = subprocess.run(command, check=True, capture_output=True, text=True, timeout=60)
    try:
        return json.loads(response.stdout)
    except ValueError as error:
        raise ValueError(f"invalid GitHub API response for {path}: {error}") from error


def _latest_ci_run(repository: str, sha: str, workflow_id: int) -> dict[str, Any] | None:
    query = urlencode({"head_sha": sha, "per_page": 100})
    pages = _gh_api(
        f"repos/{repository}/actions/workflows/{workflow_id}/runs?{query}", paginate=True
    )
    runs = [run for page in pages for run in page["workflow_runs"]]
    return latest_run(runs, sha, workflow_id)


def inspect_ci(repository: str, sha: str, workflow_id: int) -> Decision:
    run = _latest_ci_run(repository, sha, workflow_id)
    while run is not None:
        run_id, attempt = run["id"], run["run_attempt"]
        pages = _gh_api(
            f"repos/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100",
            paginate=True,
        )
        jobs = [job for page in pages for job in page["jobs"]]
        decision = run_decision(run, jobs, sha, workflow_id)
        if decision.state != "success":
            return decision
        # A rerun/newer run may have started while jobs were being fetched.
        # Do not authorize using the previous attempt's green aggregate.
        current = _latest_ci_run(repository, sha, workflow_id)
        if current is not None and (current["id"], current["run_attempt"]) == (run_id, attempt):
            return run_decision(current, jobs, sha, workflow_id)
        run = current
    return Decision("wait", f"{WORKFLOW_PATH}: no run reported for {sha}")


def _report(decision: Decision) -> int:
    prefix = "::error::" if decision.state == "failure" else ""
    print(f"{prefix}{decision.message}", flush=True)
    return 0 if decision.state == "success" else 1


def check_blocking_ci() -> int:
    try:
        return _report(needs_decision(json.loads(os.environ["CI_NEEDS"])))
    except (KeyError, ValueError) as error:
        return _report(Decision("failure", f"invalid blocking dependency results: {error}"))


def check_release_ci() -> int:
    try:
        repository, sha = os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"]
        workflow = _gh_api(f"repos/{repository}/actions/workflows/ci.yml")
        if workflow["path"] != WORKFLOW_PATH:
            return _report(Decision("failure", "ci.yml resolved to a different workflow"))
        for poll in range(30):
            decision = inspect_ci(repository, sha, workflow["id"])
            if decision.state != "wait":
                return _report(decision)
            _ = _report(decision)
            if poll < 29:
                time.sleep(60)
        return _report(Decision("failure", f"blocking CI did not complete on {sha} within 30 min"))
    except (KeyError, TypeError, ValueError, OSError, subprocess.SubprocessError) as error:
        # API errors and malformed evidence are not pending CI.
        return _report(Decision("failure", f"cannot establish blocking CI success: {error}"))
