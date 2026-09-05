"""The gate rules themselves: patterns, ratchets, and their samples.

A rule is matched against each file's **whole text**, not line by line, so a
rustfmt-wrapped call (`eprintln!(` and `"[kagi]` on different lines) is caught
— the blind spot #396 found in the old line-based gate.

Every rule carries a `sample` it must flag and, where there is a deliberate
carve-out, a `sample_ok` it must not. `check-all --selftest` asserts both, so a
rule that stopped matching anything fails loudly instead of printing OK.
"""

from __future__ import annotations

import re
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]

# Never source: build output, vendored code, tool caches.
SKIP_PARTS = frozenset({".git", "target", "vendor", "node_modules", ".claude", ".venv"})

LOC_CEILING = 800


def iter_files(globs: list[str], excludes: tuple[str, ...] = ()) -> list[Path]:
    """Repo-relative files matching any glob, minus build output and `excludes`.

    Replaces `find … -not -path …`: one traversal, identical on every OS.
    """
    seen: dict[Path, None] = {}
    for pattern in globs:
        for path in sorted(ROOT.glob(pattern)):
            if not path.is_file():
                continue
            rel = path.relative_to(ROOT)
            if SKIP_PARTS & set(rel.parts):
                continue
            if any(ex in rel.as_posix() for ex in excludes):
                continue
            seen[rel] = None
    return list(seen)


def read_text(rel: Path) -> str:
    return (ROOT / rel).read_text(encoding="utf-8", errors="replace")


@dataclass(frozen=True)
class Rule:
    """A "this pattern must not appear" gate."""

    name: str
    summary: str
    pattern: str
    globs: tuple[str, ...]
    message: str
    excludes: tuple[str, ...] = ()
    sample: str = ""
    sample_ok: str = ""
    flags: int = re.MULTILINE | re.DOTALL
    # Skip matches on `#`-comment lines. Set for rules whose subject is shell
    # or YAML, where the words being banned also appear in prose explaining
    # why they are banned (this file's own workflow comment did exactly that).
    skip_comments: bool = False

    def compiled(self) -> re.Pattern[str]:
        return re.compile(self.pattern, self.flags)

    def _matches(self, text: str) -> list[tuple[int, str]]:
        """Line number + text of every real (non-skipped) match in `text`."""
        rx = self.compiled()
        lines = text.splitlines()
        out: list[tuple[int, str]] = []
        for match in rx.finditer(text):
            line_no = text.count("\n", 0, match.start()) + 1
            line = lines[line_no - 1].strip() if lines else ""
            if self.skip_comments and line.startswith("#"):
                continue
            out.append((line_no, line))
        return out

    def fires_on(self, text: str) -> bool:
        """Would this rule fail on `text`?

        Used by both `hits` and the selftest, so a sample is judged exactly the
        way a real file is — including `skip_comments`. (Named `fires_on`, not
        `flags`: `flags` is the regex-flags field.)
        """
        return bool(self._matches(text))

    def hits(self) -> list[tuple[Path, int, str]]:
        out: list[tuple[Path, int, str]] = []
        for rel in iter_files(list(self.globs), self.excludes):
            for line_no, line in self._matches(read_text(rel)):
                out.append((rel, line_no, line))
        return out


RUST_SOURCES = ("src/**/*.rs", "crates/**/*.rs", "tests/**/*.rs", "examples/**/*.rs")

RULES: tuple[Rule, ...] = (
    Rule(
        name="ui-git2",
        summary="src/ui never uses git2 directly (ADR-0072 / ADR-0078)",
        pattern=r"git2::|Repository::open",
        globs=("src/ui/**/*.rs",),
        message=(
            "src/ui must not use git2 directly — route through kagi_git::Backend "
            "(ADR-0072 / ADR-0078)."
        ),
        sample="let repo = Repository::open(path)?;",
        sample_ok="// the backend owns git2; the UI calls kagi_git::Backend\n",
    ),
    Rule(
        name="mcp-gpui",
        summary="crates/kagi-mcp never depends on gpui (ADR-0163 / #331)",
        pattern=r"gpui *=|use +gpui|gpui::",
        globs=("crates/kagi-mcp/**/*.rs", "crates/kagi-mcp/**/Cargo.toml"),
        message="crates/kagi-mcp must not depend on gpui (ADR-0163 / #331).",
        sample="use gpui::App;",
        sample_ok="// headless by construction: no gpui in this crate\n",
    ),
    Rule(
        name="ui-core-layering",
        summary="kagi-ui-* crates never touch git2 / kagi-git (ADR-0121)",
        # Actual usage only: a path, a `use`, an `extern crate`, or a manifest
        # dependency line. The bare substring also fired on prose such as
        # "libgit2" and forced doc rewrites (#443).
        pattern=(
            r"\b(git2|kagi_git)::"
            r"|^\s*use\s+(git2|kagi_git)\b"
            r"|extern\s+crate\s+(git2|kagi_git)\b"
            r"|^(git2|kagi-git|kagi_git)\s*[=.]"
        ),
        globs=("crates/kagi-ui-*/**/*.rs", "crates/kagi-ui-*/**/Cargo.toml"),
        message="kagi-ui-* crates must not depend on git2 or kagi-git (ADR-0121).",
        sample="use kagi_git::Backend;",
        sample_ok="/// libgit2 does this differently; see the git2 token doc.\n",
    ),
    Rule(
        name="plan-verbatim",
        summary="the ADR-0129 Verbatim escape hatch stays deleted",
        pattern=r"PlanNote::Verbatim|PlanTitle::Verbatim|RecoveryKind::Verbatim",
        globs=("src/**/*.rs", "crates/**/*.rs", "tests/**/*.rs"),
        message=(
            "PlanNote::Verbatim / PlanTitle::Verbatim / RecoveryKind::Verbatim must not "
            "exist (ADR-0129 Phase 3 removed the migration escape hatch)."
        ),
        sample="PlanNote::Verbatim(text)",
        sample_ok="PlanNote::CheckoutOverlap { files }",
    ),
    Rule(
        name="modal-lists",
        summary="modal preview lists render every row (#454)",
        # `<list> … .take(` with only chain punctuation between, so the wrapped
        # form is caught too. Row-level truncation (`p.chars().take(80)`) stays
        # allowed: it shortens one line, not the list.
        pattern=r"\.(preview_files|preview_commits|skipped)\s*(\.\s*[a-z_]+\(\)\s*)*\.\s*take\(",
        globs=("src/ui/**/*.rs",),
        message=(
            "a modal preview list is truncated with .take(N) — render every row "
            "(list panel + scroll), see #454."
        ),
        sample="for f in plan\n    .preview_files\n    .iter()\n    .take(10) {}",
        sample_ok="let short: String = p.chars().take(80).collect();",
    ),
    Rule(
        name="modal-sections",
        summary="modal sections read their disclosure state (#454)",
        # A literal `open` argument makes the renderer ignore
        # `modal_section_overrides`: the caret is drawn, the click is wired, and
        # nothing happens. Shipped once, caught by a user rather than by CI.
        pattern=r"modal_section(?:_chipped)?\((?:[^;()]|\([^;]*?\))*?\b(?:true|false)\s*,",
        globs=("src/ui/**/*.rs",),
        message=(
            "a modal section passes a literal open flag — use "
            "section_open(overrides, ID, default), see #454."
        ),
        sample=(
            "body = body.child(modal_section(\n"
            "    SECTION_X,\n"
            "    Msg::A.t(),\n"
            "    3,\n"
            "    true,\n"
            "    Some(col),\n"
            "    cx,\n"
            "));"
        ),
        sample_ok=(
            "body = body.child(modal_section(\n"
            "    SECTION_X,\n"
            "    Msg::A.t(),\n"
            "    3,\n"
            "    section_open(overrides, SECTION_X, true),\n"
            "    open.then(|| col),\n"
            "    cx,\n"
            "));"
        ),
    ),
    Rule(
        name="shell-hygiene",
        summary="gates and workflows never shell out to grep/find/awk/sed -i",
        # Invocations only (word followed by an argument), so prose and job
        # names are fine. Python gate sources are not scanned: they are `.py`.
        pattern=r"(?<![\w./-])(grep|find|awk)\s+[-\"'$a-zA-Z0-9]|sed\s+-i",
        globs=("ci/**/*.sh", ".github/workflows/*.yml"),
        message=(
            "CI must not shell out to grep/find/awk/sed -i — the GNU/BSD split "
            "silently no-ops gates (#454). Add a rule in ci/src/kagi_checks/rules.py "
            "and run it with `uv run --project ci check-<name>`."
        ),
        sample="if grep -rnE 'git2::' src/ui/; then exit 1; fi",
        sample_ok="# the old gate used grep -rnE here; now it is a Python rule",
        skip_comments=True,
    ),
    Rule(
        name="uv-invocation",
        summary="workflows run the gates through uv, never a bare interpreter",
        # A bare `python3 ci/…` bypasses the lockfile and the pinned
        # interpreter, which is how "works on my machine" gets back in.
        pattern=r"(?<![\w./-])(python3?|pip3?)\s+[-\"'$a-zA-Z0-9]",
        globs=(".github/workflows/*.yml",),
        message=(
            "run the gates through uv (`uv run --frozen --project ci check-<name>`), "
            "not a bare python/pip — uv pins the interpreter and the lockfile."
        ),
        sample="run: python3 ci/gate.py check-loc",
        sample_ok="# python3 used to run the gates directly; uv does now",
        skip_comments=True,
    ),
)


# ── Ratchets: per-file counts that may shrink but never grow ─────────────────


def klog_counts() -> dict[str, int]:
    """Raw `[kagi]` emissions per file (the klog! single-channel ratchet)."""
    rx = re.compile(r'(eprintln|println)!\(\s*"\[kagi\]', re.DOTALL)
    counts: dict[str, int] = {}
    for rel in iter_files(list(RUST_SOURCES), ("crates/kagi-ui-core/src/klog.rs",)):
        found = len(rx.findall(read_text(rel)))
        if found:
            counts[rel.as_posix()] = found
    return counts


def loc_counts() -> dict[str, int]:
    """Line counts of source files over the LOC target."""
    counts: dict[str, int] = {}
    for rel in iter_files(["src/**/*.rs", "crates/*/src/**/*.rs"], ("/tests/",)):
        lines = len(read_text(rel).splitlines())
        if lines > LOC_CEILING:
            counts[rel.as_posix()] = lines
    return counts


@dataclass(frozen=True)
class Ratchet:
    name: str
    summary: str
    baseline: str
    unit: str
    guidance: str
    counts: Callable[[], dict[str, int]]


RATCHETS: tuple[Ratchet, ...] = (
    Ratchet(
        name="klog",
        summary="no new raw [kagi] lines outside klog! (ratchet, #396)",
        baseline="ci/klog-baseline.txt",
        unit='raw (e)println!("[kagi]…") call(s)',
        guidance="[kagi] contract lines must go through klog! (ADR-0096 / #396).",
        counts=klog_counts,
    ),
    Ratchet(
        name="loc",
        summary=f"no file grows past its LOC ceiling ({LOC_CEILING}, T-LOC-GATE-001)",
        baseline="ci/loc-baseline.txt",
        unit=f"LOC (>{LOC_CEILING})",
        guidance=(
            "LOC ratchet failed. Split the file(s) above, or accept the growth "
            "deliberately by raising just those ceilings in ci/loc-baseline.txt."
        ),
        counts=loc_counts,
    ),
)


# ── Custom checks: rules a single regex cannot express ──────────────────────


def ui_lateral_hits() -> list[tuple[Path, int, str, str]]:
    """Feature crates may import kagi-ui-core, never a sibling kagi-ui-* crate.

    The allowed set depends on which crate is being scanned, which is why this
    is a function and not a `Rule`. `kagi-ui-core` is the shared base, and a
    crate naming itself is not an import (ADR-0121).
    """
    # Actual usage only — a path, a `use`, an `extern crate`, or a manifest
    # dependency line. Matching the bare crate name also flagged doc comments
    # that merely *mention* a sibling ("moved here from `kagi-ui-file-history`"),
    # which is the same false-positive class #443 fixed for git2.
    rust = re.compile(
        r"\b(kagi_ui_[a-z_]+)::"
        r"|^\s*use\s+(kagi_ui_[a-z_]+)\b"
        r"|extern\s+crate\s+(kagi_ui_[a-z_]+)\b",
        re.MULTILINE,
    )
    manifest = re.compile(r"^(kagi-ui-[a-z-]+|kagi_ui_[a-z_]+)\s*[=.]", re.MULTILINE)
    out: list[tuple[Path, int, str, str]] = []
    for crate in sorted(d for d in (ROOT / "crates").glob("kagi-ui-*") if d.is_dir()):
        hyphen = crate.name
        allowed = {"kagi_ui_core", "kagi-ui-core", hyphen, hyphen.replace("-", "_")}
        for rel in iter_files([f"crates/{hyphen}/**/*.rs"]):
            text = read_text(rel)
            lines = text.splitlines()
            for match in rust.finditer(text):
                name = next(g for g in match.groups() if g)
                if name in allowed:
                    continue
                line_no = text.count("\n", 0, match.start()) + 1
                out.append((rel, line_no, lines[line_no - 1].strip(), hyphen))
        for rel in iter_files([f"crates/{hyphen}/**/Cargo.toml"]):
            text = read_text(rel)
            lines = text.splitlines()
            for match in manifest.finditer(text):
                if match.group(1) in allowed:
                    continue
                line_no = text.count("\n", 0, match.start()) + 1
                out.append((rel, line_no, lines[line_no - 1].strip(), hyphen))
    return out


def ui_lateral_crate_count() -> int:
    return len([d for d in (ROOT / "crates").glob("kagi-ui-*") if d.is_dir()])
