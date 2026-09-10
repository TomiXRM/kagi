"""Summarise `backend_probe` runs for the #627 cross-platform evidence.

Absolute timings are a property of the runner, not of Git. GitHub's Linux and
Windows machines differ in CPU, disk and filesystem, so comparing milliseconds
*between* operating systems measures the hardware. Two things do carry across:

* whether the two backends return the **same canonical output** on that OS, and
* the **within-runner ratio** between them, since both ran on one machine.

This tool reports exactly those two and refuses to emit a cross-OS comparison.
The caveat is written into the JSON as well as the console output, because a
value separated from its condition is how #627 produced three wrong readings
(`RPT-627 §5.2`).
"""

from __future__ import annotations

import json
import platform
import sys
from pathlib import Path
from typing import Any

# The probe's own warm-up contract: the first two iterations are discarded.
WARMUP_ITERATIONS = 2

PORTABILITY_NOTE = (
    "Absolute times are runner-specific. Do NOT compare milliseconds across "
    "operating systems — the runners differ in hardware. Canonical-output "
    "equality is the portable result. The CLI/libgit2 ratio is a within-runner "
    "observation only: it also moves with the runner (measured 2.9x-10.4x for "
    "the same fixture across three runners), so it is not comparable between "
    "operating systems either."
)


def _percentile(values: list[float], fraction: float) -> float:
    """Nearest-rank percentile, matching how the P4 series reported p95."""
    if not values:
        return 0.0
    rank = max(1, min(len(values), int(-(-len(values) * fraction // 1))))
    return sorted(values)[rank - 1]


def _series(path: Path) -> dict[str, Any]:
    report = json.loads(path.read_text())
    iterations = report.get("iterations", [])[WARMUP_ITERATIONS:]
    walls = [it["timing"]["wall_ns"] / 1e6 for it in iterations if it.get("timing")]
    canonical = [it.get("canonical_output") for it in iterations]
    return {
        "backend": report.get("backend"),
        "operation": report.get("operation"),
        "n": len(walls),
        "median_ms": _percentile(walls, 0.5),
        "p95_ms": _percentile(walls, 0.95),
        "min_ms": min(walls) if walls else 0.0,
        "max_ms": max(walls) if walls else 0.0,
        "series_setup": report.get("series_setup"),
        "canonical": canonical[0] if canonical else None,
        "canonical_stable": all(c == canonical[0] for c in canonical),
    }


def _semantic(canonical: Any) -> Any:
    """The part of a probe's output that means "what Git reported".

    A blacklist of descriptive keys would break every time a probe module adds
    one (`candidate`, `fsmonitor`, `git_processes` all describe *how* a backend
    ran, not what it found). The harness already separates the two: probes put
    their comparable payload under `canonical`, and the snapshot probe uses
    `information`. Compare that and nothing else.

    `missing_information` is deliberately excluded. The CLI composite not
    returning some fields is a real and known asymmetry (`RPT-627 F-07`), and it
    is reported on its own rather than as a mismatch that hides the payload.
    """
    if not isinstance(canonical, dict):
        return canonical
    for key in ("canonical", "information"):
        if key in canonical:
            return canonical[key]
    raise KeyError(
        "probe output has neither 'canonical' nor 'information'; "
        "the probe module must expose a comparable payload"
    )


def _compare(left: Any, right: Any) -> tuple[bool | None, dict[str, Any], list[str], int]:
    """Compare two semantic payloads, reporting what could not be compared.

    A field one backend reports and the other leaves `null` is **not a
    difference** — it is an axis on which the two cannot be compared, exactly as
    P3 treated recovery handles (direct Git CLI writes no Kagi oplog, so the
    handle axis is 比較不能 rather than equal). Counting those as divergence
    would drown the real finding; counting them as equality would invent one.

    Returns `(equal, differing, not_compared, compared_count)`, where `equal`
    is `None` when no field could be compared at all.
    """
    if not isinstance(left, dict) or not isinstance(right, dict):
        return (left == right, {}, [], 1)

    differing: dict[str, Any] = {}
    not_compared: list[str] = []
    compared = 0
    for key in sorted(set(left) | set(right)):
        lhs, rhs = left.get(key), right.get(key)
        if lhs is None or rhs is None:
            not_compared.append(key)
            continue
        compared += 1
        if lhs != rhs:
            differing[key] = {"libgit2": lhs, "cli": rhs}
    if compared == 0:
        return (None, {}, not_compared, 0)
    return (not differing, differing, not_compared, compared)


def probe_report() -> int:
    # Windows defaults stdout to the ANSI code page, so the em dash in
    # PORTABILITY_NOTE was written as cp1252 and the artifact was not valid
    # UTF-8 (#627: the Windows summary failed to parse). The report is JSON, and
    # JSON is UTF-8, so say so rather than dropping the punctuation.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")

    paths = [Path(arg) for arg in sys.argv[1:]]
    if not paths:
        print("usage: probe-report <probe-json>...", file=sys.stderr)
        return 2

    runs = [_series(path) for path in paths]
    by_operation: dict[str, dict[str, dict[str, Any]]] = {}
    for run in runs:
        by_operation.setdefault(run["operation"], {})[run["backend"]] = run

    results = []
    divergent = 0
    for operation, backends in sorted(by_operation.items()):
        entry: dict[str, Any] = {"operation": operation}
        for name, run in backends.items():
            entry[name] = {
                key: run[key] for key in ("n", "median_ms", "p95_ms", "min_ms", "max_ms")
            }
            if not run["canonical_stable"]:
                entry.setdefault("warnings", []).append(
                    f"{name}: canonical output changed between iterations"
                )
        libgit2, cli = backends.get("libgit2"), backends.get("cli")
        if libgit2 and cli:
            if libgit2["median_ms"]:
                entry["ratio_cli_over_libgit2"] = round(cli["median_ms"] / libgit2["median_ms"], 3)
            equal, differing, skipped, compared_count = _compare(
                _semantic(libgit2["canonical"]), _semantic(cli["canonical"])
            )
            entry["canonical_equal"] = equal
            entry["fields_compared"] = compared_count
            if skipped:
                entry["not_compared"] = skipped
            if equal is False:
                divergent += 1
                entry["differing_fields"] = differing
            for name, run in (("libgit2", libgit2), ("cli", cli)):
                missing = (run["canonical"] or {}).get("missing_information")
                if missing:
                    entry.setdefault("missing_information", {})[name] = missing
        results.append(entry)

    document = {
        "runner": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
        },
        "portability_note": PORTABILITY_NOTE,
        "warmup_iterations_discarded": WARMUP_ITERATIONS,
        "results": results,
    }
    print(json.dumps(document, indent=2, ensure_ascii=False))

    print(f"\n{PORTABILITY_NOTE}\n", file=sys.stderr)
    for entry in results:
        equal = entry.get("canonical_equal")
        if "libgit2" not in entry or "cli" not in entry:
            verdict = "single-backend"
        else:
            verdict = {True: "same", False: "DIFFERENT", None: "not comparable"}[equal]
        # How much was actually compared matters as much as the verdict: "same"
        # over one of eight fields is a much weaker statement than "same" over
        # all of them, and the console is where that gets skimmed.
        skipped_fields: list[str] = list(entry.get("not_compared") or [])
        compared = int(entry.get("fields_compared") or 0)
        if skipped_fields:
            total = len(skipped_fields) + compared
            verdict += f" ({len(skipped_fields)} of {total} field(s) not comparable)"
        ratio = entry.get("ratio_cli_over_libgit2")
        ratio_text = f"  CLI/libgit2 = {ratio}" if ratio else ""
        print(f"  {entry['operation']}: canonical {verdict}{ratio_text}", file=sys.stderr)

    # A semantic divergence is the finding this job exists to surface, so it
    # fails loudly rather than sitting in an artifact nobody opens.
    if divergent:
        print(
            f"\n{divergent} operation(s) returned different canonical output "
            f"between backends on this runner.",
            file=sys.stderr,
        )
        return 1
    return 0
