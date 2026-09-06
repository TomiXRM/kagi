"""Validate repository paths named by the canonical verification skill."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory

SKILL = Path(".claude/skills/verify/SKILL.md")

# Only these Markdown forms are executable or intentionally linked references.
# Plain prose is deliberately excluded so descriptions such as ``scripts/*``
# remain documentation, rather than an accidental incomplete path reference.
FENCE = re.compile(
    r"(?ms)^ {0,3}(?P<fence>`{3,}|~{3,})[^\n]*\n"
    r"(?P<body>.*?)(?:^ {0,3}(?P=fence)[ \t]*$)"
)
INLINE = re.compile(r"(?<!`)`([^`\n]+)`(?!`)")
LINK = re.compile(r"!?\[[^\]]*\]\(\s*<?([^\s)>]+)")
TOKEN = re.compile(r"(?<![A-Za-z0-9_./-])(?P<token>(?:scripts|tests)/[^\s`'\"()]+)")
LINE = re.compile(r":\d+(?::\d+)?$")
RUST_TEST = re.compile(r"(?:^|/)[^/]+\.rs(?:$|[./])")


@dataclass(frozen=True)
class Reference:
    path: str
    line: int


def _fragments(text: str) -> list[tuple[str, int]]:
    """Return fenced code, inline code, and Markdown link destinations."""
    fragments: list[tuple[str, int]] = []
    fenced_spans: list[tuple[int, int]] = []
    for match in FENCE.finditer(text):
        fragments.append((match.group("body"), text.count("\n", 0, match.start("body")) + 1))
        fenced_spans.append(match.span())
    without_fences = list(text)
    for start, end in fenced_spans:
        without_fences[start:end] = " " * (end - start)
    remaining = "".join(without_fences)
    for match in INLINE.finditer(remaining):
        fragments.append((match.group(1), text.count("\n", 0, match.start(1)) + 1))
    for match in LINK.finditer(text):
        fragments.append((match.group(1), text.count("\n", 0, match.start(1)) + 1))
    return fragments


def references(text: str) -> list[Reference]:
    """Concrete skill paths, with source line numbers for useful diagnostics."""
    found: set[Reference] = set()
    for fragment, first_line in _fragments(text):
        for match in TOKEN.finditer(fragment):
            path = match.group("token").rstrip(".,;")
            path = LINE.sub("", path.split("#", 1)[0])
            if not any(marker in path for marker in "*?{}<>[]") and (
                not path.startswith("tests/") or RUST_TEST.search(path)
            ):
                found.add(Reference(path, first_line + fragment.count("\n", 0, match.start())))
    return sorted(found, key=lambda reference: (reference.line, reference.path))


def issues(root: Path) -> list[str]:
    """Missing canonical skill/read failures and missing named paths."""
    source = root / SKILL
    try:
        text = source.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [f"cannot read canonical skill {SKILL}: {error}"]
    return [
        f"{SKILL}:{reference.line}: referenced path does not exist: {reference.path}"
        for reference in references(text)
        if not (root / reference.path).is_file()
    ]


def selftest() -> list[str]:
    """Exercise the real filesystem checker with an isolated skill fixture."""
    with TemporaryDirectory() as directory:
        root = Path(directory)
        skill = root / SKILL
        (root / "scripts").mkdir(parents=True)
        (root / "tests/nested").mkdir(parents=True)
        (root / "scripts/existing.sh").write_text("", encoding="utf-8")
        (root / "scripts/between.sh").write_text("", encoding="utf-8")
        (root / "tests/nested/existing.rs").write_text("", encoding="utf-8")
        (root / "tests/nested/fenced.rs").write_text("", encoding="utf-8")
        (root / "tests/nested/linked.rs").write_text("", encoding="utf-8")
        skill.parent.mkdir(parents=True)
        existing = (
            "`scripts/existing.sh`\n"
            "```bash\n"
            "cargo test --test tests/nested/existing.rs:12\n"
            "```\n"
            "`scripts/between.sh`\n"
            "```bash\n"
            "cargo test --test tests/nested/fenced.rs\n"
            "```\n"
            "[linked runner](tests/nested/linked.rs#scenario)\n"
        )
        skill.write_text(existing, encoding="utf-8")
        if len(list(FENCE.finditer(existing))) != 2:
            return ["fixture did not keep its two fenced blocks separate"]
        expected = {
            "scripts/existing.sh",
            "scripts/between.sh",
            "tests/nested/existing.rs",
            "tests/nested/fenced.rs",
            "tests/nested/linked.rs",
        }
        if {reference.path for reference in references(existing)} != expected:
            return ["fixture did not extract its inline, fenced, and linked references"]
        if issues(root):
            return ["fixture reported a missing path when every extracted path exists"]

        skill.write_text(
            existing
            + "`scripts/missing.sh`\n"
            + "`tests/nested/missing.rs`\n"
            + "`tests/nested/existing.rs.missing`\n"
            + "`scripts/existing.sh/nope`\n"
            + "`scripts/existing.sh*` `tests/**/*.rs` `scripts/{runner}.sh`\n"
            + "`scripts/prefix-<name>.sh` `scripts/run[12].sh` `tests/not-rust.txt`\n"
            + "`/scripts/missing.sh` `https://example.invalid/scripts/missing.sh`\n",
            encoding="utf-8",
        )
        found = issues(root)
        missing_paths = {issue.rsplit(": ", 1)[-1] for issue in found}
        expected_missing = {
            "scripts/missing.sh",
            "tests/nested/missing.rs",
            "tests/nested/existing.rs.missing",
            "scripts/existing.sh/nope",
        }
        if missing_paths != expected_missing:
            return [f"fixture did not report exactly its missing full paths: {found}"]

        skill.unlink()
        missing = issues(root)
        if len(missing) != 1 or "cannot read canonical skill" not in missing[0]:
            return [f"missing canonical skill was not an explicit failure: {missing}"]
    return []
