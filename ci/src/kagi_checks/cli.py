"""Entry points: one `check-<name>` command per gate, plus `check-all`.

    uv run --project ci check-all
    uv run --project ci check-all --selftest
    uv run --project ci check-modal-sections
    uv run --project ci check-loc --write-baseline

Every command exits 0 on pass and 1 on failure, printing GitHub's `::error::`
annotations so a red gate points at the offending line in the PR diff.
"""

from __future__ import annotations

import sys
from pathlib import Path

from kagi_checks.rules import (
    RATCHETS,
    RULES,
    Ratchet,
    Rule,
    ui_lateral_crate_count,
    ui_lateral_hits,
)

ROOT = Path(__file__).resolve().parents[3]


def _run_rule(rule: Rule) -> int:
    hits = rule.hits()
    if not hits:
        print(f"OK: {rule.name} — {rule.summary}.")
        return 0
    for rel, line_no, line in hits:
        print(f"{rel}:{line_no}: {line}")
    print(f"::error::{rule.message}")
    return 1


def _rule(name: str) -> Rule:
    for rule in RULES:
        if rule.name == name:
            return rule
    raise KeyError(name)


def _read_baseline(path: Path) -> dict[str, int]:
    if not path.exists():
        return {}
    out: dict[str, int] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        name, count = line.rsplit(" ", 1)
        out[name] = int(count)
    return out


def _run_ratchet(ratchet: Ratchet, write: bool) -> int:
    path = ROOT / ratchet.baseline
    counts = ratchet.counts()
    if write:
        path.write_text("".join(f"{k} {v}\n" for k, v in sorted(counts.items())), encoding="utf-8")
        print(f"wrote {ratchet.baseline} ({len(counts)} entries)")
        return 0

    baseline = _read_baseline(path)
    regen = f"regenerate: uv run --project ci check-{ratchet.name} --write-baseline"
    failed = False
    for name, count in sorted(counts.items()):
        allowed = baseline.get(name, 0)
        if count > allowed:
            if name in baseline:
                print(
                    f"::error::{name} grew to {count} {ratchet.unit}, exceeding its "
                    f"baseline of {allowed} in {ratchet.baseline}."
                )
            else:
                print(
                    f"::error::{name} has {count} {ratchet.unit} and is not in {ratchet.baseline}."
                )
            failed = True
        elif count < allowed:
            print(
                f"::notice::{name} shrank to {count} {ratchet.unit} (baseline {allowed}) — {regen}"
            )
    for name, allowed in sorted(baseline.items()):
        if name not in counts and allowed:
            print(f"::notice::{name} no longer appears (baseline {allowed}) — {regen}")
    if failed:
        print(f"::error::{ratchet.guidance}")
        return 1
    print(f"OK: {ratchet.name} — {ratchet.summary}.")
    return 0


def _ratchet(name: str) -> Ratchet:
    for ratchet in RATCHETS:
        if ratchet.name == name:
            return ratchet
    raise KeyError(name)


def _wants(flag: str) -> bool:
    return flag in sys.argv[1:]


# ── Pattern rules ───────────────────────────────────────────────────────────


def check_ui_git2() -> int:
    return _run_rule(_rule("ui-git2"))


def check_mcp_gpui() -> int:
    return _run_rule(_rule("mcp-gpui"))


def check_ui_core_layering() -> int:
    return _run_rule(_rule("ui-core-layering"))


def check_plan_verbatim() -> int:
    return _run_rule(_rule("plan-verbatim"))


def check_modal_lists() -> int:
    return _run_rule(_rule("modal-lists"))


def check_modal_sections() -> int:
    return _run_rule(_rule("modal-sections"))


def check_shell_hygiene() -> int:
    status = _run_rule(_rule("shell-hygiene"))
    return status | _run_rule(_rule("uv-invocation"))


# ── Custom check ────────────────────────────────────────────────────────────


def check_ui_lateral() -> int:
    hits = ui_lateral_hits()
    if not hits:
        print(
            f"OK: ui-lateral — no lateral imports across {ui_lateral_crate_count()} "
            "kagi-ui-* crates."
        )
        return 0
    for rel, line_no, line, crate in hits:
        print(f"{rel}:{line_no}: {line}  ({crate})")
    print(
        "::error::a kagi-ui-* crate imports a sibling feature crate — feature crates "
        "may only depend on kagi-ui-core (ADR-0121)."
    )
    return 1


# ── Ratchets ────────────────────────────────────────────────────────────────


def check_klog() -> int:
    return _run_ratchet(_ratchet("klog"), _wants("--write-baseline"))


def check_loc() -> int:
    return _run_ratchet(_ratchet("loc"), _wants("--write-baseline"))


# ── Everything ──────────────────────────────────────────────────────────────


def selftest() -> int:
    """Prove each rule still flags its own sample.

    The failure this guards: a pattern that matches nothing (a bad escape, or
    BSD grep's 255-repetition cap in the shell era) still prints OK, so the
    gate is green while checking nothing.
    """
    failed = False
    for rule in RULES:
        if not rule.sample:
            print(f"::error::rule {rule.name} has no positive sample — rules must be testable.")
            failed = True
            continue
        if not rule.fires_on(rule.sample):
            print(
                f"::error::rule {rule.name} no longer matches its positive sample:\n{rule.sample}"
            )
            failed = True
        if rule.sample_ok and rule.fires_on(rule.sample_ok):
            print(
                f"::error::rule {rule.name} matches its negative sample "
                f"(false positive):\n{rule.sample_ok}"
            )
            failed = True
    for ratchet in RATCHETS:
        if not (ROOT / ratchet.baseline).exists():
            print(f"::error::ratchet {ratchet.name} has no baseline at {ratchet.baseline}.")
            failed = True
    if failed:
        return 1
    print(f"OK: {len(RULES)} rules match their samples; {len(RATCHETS)} ratchets have baselines.")
    return 0


def check_all() -> int:
    if _wants("--selftest"):
        return selftest()
    status = selftest()
    for rule in RULES:
        status |= _run_rule(rule)
    status |= check_ui_lateral()
    for ratchet in RATCHETS:
        status |= _run_ratchet(ratchet, False)
    return status


if __name__ == "__main__":
    sys.exit(check_all())
