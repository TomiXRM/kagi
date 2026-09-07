"""Check literal busy tags and finite operation-name producers against EN/JA labels."""

from __future__ import annotations

import re
from pathlib import Path

TABLE = "crates/kagi-ui-core/src/i18n/busy.rs"
LITERAL = re.compile(
    r'\bbusy_op\s*=\s*Some\(\s*"([^"\n]+)"'
    r'|\b(?:reserve_write|mark_write_busy)\(\s*"([^"\n]+)"'
)


def label_names(source: str) -> set[str]:
    table = re.search(r"const LABELS:.*?= &\[(.*?)\];", source, re.DOTALL)
    if table is None:
        return set()
    return set(re.findall(r'\(\s*"([^"\n]+)"\s*,', table[1]))


def missing_tags(source: str, known: set[str]) -> list[str]:
    return [
        tag
        for match in LITERAL.finditer(source)
        if (tag := next(value for value in match.groups() if value is not None)) not in known
    ]


def issues(root: Path) -> list[str]:
    table = root / TABLE
    if not table.is_file():
        return [f"busy-labels: missing label table {TABLE}"]
    known = label_names(table.read_text())
    if not known:
        return ["busy-labels: empty EN/JA label table"]
    result = []
    sources = list((root / "src/ui").rglob("*.rs"))
    if not sources:
        return ["busy-labels: no UI source files found"]
    for path in sources:
        for tag in missing_tags(path.read_text(), known):
            result.append(f"{path.relative_to(root)}: busy tag {tag!r} has no EN/JA label")
    # Dynamic names come from these finite match tables: backend operations
    # (including stash), branch-plan jobs, staging and dispatch_job families.
    producers = {
        "crates/kagi-domain/src/operation.rs": r'=>\s*"([^"\n]+)"',
        "src/ui/operations/staging_failure.rs": r'=>\s*"([^"\n]+)"',
        "src/ui/operations/branch.rs": r'BranchPlanKind::\w+\s*=>\s*"([^"\n]+)"',
        "src/ui/operations/app_bridge.rs": r'\(\s*"([^"\n]+)"\s*,\s*Msg::',
    }
    for name, pattern in producers.items():
        path = root / name
        if not path.is_file():
            result.append(f"busy-labels: missing operation producer {name}")
            continue
        source = path.read_text()
        if name.endswith("app_bridge.rs"):
            dispatch = re.search(r"let \(name, label\) = match .*?\n        };", source, re.DOTALL)
            source = dispatch[0] if dispatch else ""
        tags = re.findall(pattern, source)
        if not tags:
            result.append(f"busy-labels: no finite operation names found in {name}")
        for tag in sorted(set(tags) - known):
            result.append(f"{name}: operation tag {tag!r} has no EN/JA busy label")
    return result


def selftest() -> list[str]:
    known = {"fetch", "commit"}
    samples = [
        ('self.busy_op = Some("fetch");', []),
        ('self.reserve_write("commit", &repo, cx)', []),
        ('self.busy_op = Some("typo-fetch");', ["typo-fetch"]),
        ('self.reserve_write(\n "app-writer", &repo, cx)', ["app-writer"]),
        ('self.mark_write_busy("new-operation");', ["new-operation"]),
    ]
    if label_names(
        'const LABELS: &[(&str, &str, &str)] = &[("fetch", "EN", "JA")];\n'
        'fn test() { ("typo", "EN", "JA"); }'
    ) != {"fetch"}:
        return ["label parser must ignore test-only table entries"]
    return [
        f"wrong busy tag result for {source!r}"
        for source, expected in samples
        if missing_tags(source, known) != expected
    ]
