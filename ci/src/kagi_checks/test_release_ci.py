"""Offline GitHub API/result fixtures for the release gate (#483)."""

from __future__ import annotations

import io
import os
import subprocess
import unittest
from contextlib import redirect_stdout
from typing import Any
from unittest.mock import patch

from . import release_ci

SHA = "a" * 40
WORKFLOW_ID = 17
REPOSITORY = "fixture/kagi"


def ci_run(**overrides: Any) -> dict[str, Any]:
    return {
        "id": 101,
        "run_number": 10,
        "run_attempt": 1,
        "workflow_id": WORKFLOW_ID,
        "head_sha": SHA,
        "status": "completed",
        "conclusion": "success",
        **overrides,
    }


def aggregate(run: dict[str, Any], **overrides: Any) -> dict[str, Any]:
    return {
        "name": release_ci.BLOCKING_JOB,
        "run_id": run["id"],
        "run_attempt": run["run_attempt"],
        "head_sha": run["head_sha"],
        "status": "completed",
        "conclusion": "success",
        **overrides,
    }


def runs_page(*runs: dict[str, Any]) -> list[dict[str, Any]]:
    return [{"workflow_runs": list(runs)}]


class DependencyFixtures(unittest.TestCase):
    def test_failed_gates_lint_blocks_release_despite_other_successes(self) -> None:
        needs = {
            "invariants": {"result": "success"},
            "gates-lint": {"result": "failure"},
            "test-macos": {"result": "success"},
        }
        decision = release_ci.needs_decision(needs)
        run = ci_run()
        self.assertEqual(decision.state, "failure")
        self.assertEqual(
            release_ci.run_decision(
                run, [aggregate(run, conclusion=decision.state)], SHA, WORKFLOW_ID
            ).state,
            "failure",
        )

    def test_incomplete_matrix_dependency_never_authorizes_release(self) -> None:
        # GitHub folds all invariant matrix legs into needs.invariants.result.
        # An absent leg cannot provide a successful completed matrix result.
        for result in (None, "pending", "skipped", "cancelled", "failure"):
            with self.subTest(result=result):
                needs = {
                    "invariants": {} if result is None else {"result": result},
                    "gates-lint": {"result": "success"},
                    "test-macos": {"result": "success"},
                }
                self.assertEqual(release_ci.needs_decision(needs).state, "failure")

    def test_dependency_names_do_not_define_the_required_set_in_python(self) -> None:
        needs = {"renamed-matrix": {"result": "success"}, "new-gate": {"result": "success"}}
        self.assertEqual(release_ci.needs_decision(needs).state, "success")
        needs["new-gate"]["result"] = "failure"
        self.assertEqual(release_ci.needs_decision(needs).state, "failure")

    def test_missing_dependency_evidence_fails_closed(self) -> None:
        for needs in ({}, None, {"invariants": None}):
            with self.subTest(needs=needs):
                self.assertEqual(release_ci.needs_decision(needs).state, "failure")


class RunFixtures(unittest.TestCase):
    def test_newest_run_wins_even_when_older_run_was_rerun(self) -> None:
        older = ci_run(run_attempt=5, conclusion="failure")
        newer = ci_run(id=102, run_number=11)
        selected = release_ci.latest_run([newer, older], SHA, WORKFLOW_ID)
        self.assertEqual(selected, newer)
        self.assertEqual(
            release_ci.run_decision(newer, [aggregate(newer)], SHA, WORKFLOW_ID).state, "success"
        )

    def test_newer_failure_cannot_fall_back_to_older_green_run(self) -> None:
        older = ci_run()
        newer = ci_run(id=102, run_number=11, conclusion="failure")
        selected = release_ci.latest_run([older, newer], SHA, WORKFLOW_ID)
        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(
            release_ci.run_decision(
                selected, [aggregate(newer, conclusion="failure")], SHA, WORKFLOW_ID
            ).state,
            "failure",
        )

    def test_wrong_sha_or_workflow_cannot_supply_same_named_aggregate(self) -> None:
        for wrong in (ci_run(head_sha="b" * 40), ci_run(workflow_id=18)):
            with self.subTest(run=wrong):
                self.assertIsNone(release_ci.latest_run([wrong], SHA, WORKFLOW_ID))
                self.assertEqual(
                    release_ci.run_decision(wrong, [aggregate(wrong)], SHA, WORKFLOW_ID).state,
                    "failure",
                )

    def test_only_latest_attempt_counts(self) -> None:
        old = ci_run()
        latest = ci_run(run_attempt=2)
        old_failure = aggregate(old, conclusion="failure")
        new_success = aggregate(latest)
        self.assertEqual(
            release_ci.run_decision(latest, [old_failure, new_success], SHA, WORKFLOW_ID).state,
            "success",
        )
        self.assertEqual(
            release_ci.run_decision(latest, [aggregate(old)], SHA, WORKFLOW_ID).state, "failure"
        )
        latest.update(status="in_progress", conclusion=None)
        self.assertEqual(
            release_ci.run_decision(latest, [aggregate(old)], SHA, WORKFLOW_ID).state, "wait"
        )

    def test_wrong_run_or_sha_job_is_not_evidence(self) -> None:
        run = ci_run()
        for job in (aggregate(run, run_id=999), aggregate(run, head_sha="b" * 40)):
            with self.subTest(job=job):
                self.assertEqual(
                    release_ci.run_decision(run, [job], SHA, WORKFLOW_ID).state, "failure"
                )

    def test_terminal_aggregate_conclusions_fail_immediately(self) -> None:
        run = ci_run(status="in_progress", conclusion=None)
        for conclusion in ("failure", "skipped", "cancelled", "timed_out", "neutral", None):
            with self.subTest(conclusion=conclusion):
                self.assertEqual(
                    release_ci.run_decision(
                        run, [aggregate(run, conclusion=conclusion)], SHA, WORKFLOW_ID
                    ).state,
                    "failure",
                )

    def test_only_absent_or_in_progress_work_waits(self) -> None:
        run = ci_run(status="in_progress", conclusion=None)
        self.assertEqual(release_ci.run_decision(run, [], SHA, WORKFLOW_ID).state, "wait")
        pending = aggregate(run, status="queued", conclusion=None)
        self.assertEqual(release_ci.run_decision(run, [pending], SHA, WORKFLOW_ID).state, "wait")
        run.update(status="completed", conclusion="cancelled")
        self.assertEqual(release_ci.run_decision(run, [], SHA, WORKFLOW_ID).state, "failure")
        self.assertEqual(release_ci.run_decision(run, [pending], SHA, WORKFLOW_ID).state, "failure")

    def test_advisory_jobs_do_not_block_green_aggregate(self) -> None:
        run = ci_run(status="in_progress", conclusion=None)
        jobs = [
            aggregate(run),
            aggregate(run, name="test (Linux, advisory)", conclusion="failure"),
            aggregate(run, name="fmt + clippy (advisory)", status="queued", conclusion=None),
        ]
        self.assertEqual(release_ci.run_decision(run, jobs, SHA, WORKFLOW_ID).state, "success")


class ApiFixtures(unittest.TestCase):
    def test_attempt_endpoint_and_all_pages_supply_release_evidence(self) -> None:
        run = ci_run(run_attempt=2)
        runs_path = (
            f"repos/{REPOSITORY}/actions/workflows/{WORKFLOW_ID}/runs?head_sha={SHA}&per_page=100"
        )
        responses = {
            f"repos/{REPOSITORY}/actions/workflows/ci.yml": {
                "id": WORKFLOW_ID,
                "path": release_ci.WORKFLOW_PATH,
            },
            runs_path: [
                {"workflow_runs": []},
                {"workflow_runs": [run]},
            ],
            f"repos/{REPOSITORY}/actions/runs/101/attempts/2/jobs?per_page=100": [
                {"jobs": []},
                {"jobs": [aggregate(run)]},
            ],
        }

        def api_response(path: str, *, paginate: bool = False) -> Any:
            response = responses[path]
            # Without pagination gh returns the first page object, not a page list.
            return response[0] if isinstance(response, list) and not paginate else response

        with (
            patch.dict(os.environ, GITHUB_REPOSITORY=REPOSITORY, GITHUB_SHA=SHA),
            patch.object(release_ci, "_gh_api", side_effect=api_response),
            patch("kagi_checks.release_ci.time.sleep") as sleep,
            redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(release_ci.check_release_ci(), 0)
        sleep.assert_not_called()

    def test_rerun_started_during_query_cannot_use_previous_success(self) -> None:
        old = ci_run()
        current = ci_run(run_attempt=2, conclusion="failure")
        responses = [
            runs_page(old),
            [{"jobs": [aggregate(old)]}],
            runs_page(current),
            [{"jobs": [aggregate(current, conclusion="failure")]}],
        ]
        with patch.object(release_ci, "_gh_api", side_effect=responses):
            self.assertEqual(release_ci.inspect_ci(REPOSITORY, SHA, WORKFLOW_ID).state, "failure")

    def test_failed_aggregate_is_not_retried(self) -> None:
        run = ci_run(conclusion="failure")
        responses = [
            {"id": WORKFLOW_ID, "path": release_ci.WORKFLOW_PATH},
            runs_page(run),
            [{"jobs": [aggregate(run, conclusion="failure")]}],
        ]
        with (
            patch.dict(os.environ, GITHUB_REPOSITORY=REPOSITORY, GITHUB_SHA=SHA),
            patch.object(release_ci, "_gh_api", side_effect=responses),
            patch("kagi_checks.release_ci.time.sleep") as sleep,
            redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(release_ci.check_release_ci(), 1)
        sleep.assert_not_called()

    def test_absent_ci_times_out_without_authorizing(self) -> None:
        responses = [{"id": WORKFLOW_ID, "path": release_ci.WORKFLOW_PATH}, *[runs_page()] * 30]
        with (
            patch.dict(os.environ, GITHUB_REPOSITORY=REPOSITORY, GITHUB_SHA=SHA),
            patch.object(release_ci, "_gh_api", side_effect=responses),
            patch("kagi_checks.release_ci.time.sleep"),
            redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(release_ci.check_release_ci(), 1)

    def test_workflow_identity_and_api_errors_fail_without_waiting(self) -> None:
        for response in (
            {"id": WORKFLOW_ID, "path": ".github/workflows/impostor.yml"},
            subprocess.CalledProcessError(1, "gh api"),
            {},
        ):
            with (
                self.subTest(response=response),
                patch.dict(os.environ, GITHUB_REPOSITORY=REPOSITORY, GITHUB_SHA=SHA),
                patch.object(release_ci, "_gh_api", side_effect=[response]),
                patch("kagi_checks.release_ci.time.sleep") as sleep,
                redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(release_ci.check_release_ci(), 1)
                sleep.assert_not_called()


if __name__ == "__main__":
    _ = unittest.main()
