"""The gate rules themselves: patterns, ratchets, and their samples.

A rule is matched against each file's **whole text**, not line by line, so a
rustfmt-wrapped call (`eprintln!(` and `"[kagi]` on different lines) is caught
— the blind spot #396 found in the old line-based gate.

Cargo manifests are *parsed*, never matched: `backend = { package = "kagi-git" }`
is a kagi-git dependency however the key reads, and a regex over dependency
lines misses it (and quoted keys, and `[dependencies.kagi-git]` tables) — after
which the Rust side imports `backend::Backend` and the source patterns miss it
too. Sources are matched, manifests are parsed.

Every gate carries `samples` it must flag and, where there is a deliberate
carve-out, `samples_ok` it must not; every ratchet carries samples with the
count its counter must produce. `check-all --selftest` asserts all of it, so a
gate that stopped matching anything fails loudly instead of printing OK — for a
ratchet that means a counter matching nothing, which would report every file as
shrunk and never fail.
"""

from __future__ import annotations

import re
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

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
    samples: tuple[str, ...] = ()
    samples_ok: tuple[str, ...] = ()
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


# Every Rust source in the repo. `xtask/` included: the shell-era klog gate ran
# `grep --include='*.rs' .` over the whole tree, and the first glob list here
# quietly dropped the build tooling from that scope.
RUST_SOURCES = (
    "src/**/*.rs",
    "crates/**/*.rs",
    "tests/**/*.rs",
    "examples/**/*.rs",
    "xtask/**/*.rs",
)

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
        samples=("let repo = Repository::open(path)?;",),
        samples_ok=("// the backend owns git2; the UI calls kagi_git::Backend\n",),
    ),
    Rule(
        name="mcp-gpui",
        summary="crates/kagi-mcp never uses gpui (ADR-0163 / #331)",
        # Sources only; the manifest side is `mcp-gpui-manifest`, which parses
        # the dependency tables instead of matching their lines.
        pattern=r"^\s*use\s+gpui\b|gpui::",
        globs=("crates/kagi-mcp/**/*.rs",),
        message="crates/kagi-mcp must not depend on gpui (ADR-0163 / #331).",
        samples=("use gpui::App;",),
        samples_ok=("// headless by construction: no gpui in this crate\n",),
    ),
    Rule(
        name="klog-raw",
        summary="no same-line raw [kagi] emission (ADR-0096)",
        # Same-line only: `[ \t]*`, never `\s*`, because `\s` spans newlines and
        # the rustfmt-wrapped form is the pre-existing debt the `klog` ratchet
        # tolerates by count. Zero tolerance here is what the ratchet alone
        # cannot give: a file at its baseline could convert one wrapped call to
        # `klog!` and add a fresh same-line one at an unchanged count (round-2
        # review finding).
        pattern=r'(?:eprintln|println)!\([ \t]*"\[kagi\]',
        globs=RUST_SOURCES,
        excludes=("crates/kagi-ui-core/src/klog.rs",),
        message=(
            "a [kagi] contract line is emitted directly — route it through klog! "
            "(ADR-0096); the ratchet only tolerates the pre-existing wrapped calls."
        ),
        samples=('eprintln!("[kagi] refreshed");',),
        samples_ok=(
            'klog!("refreshed");',
            'eprintln!(\n    "[kagi] refreshed"\n);',
        ),
        flags=re.MULTILINE,
    ),
    Rule(
        name="ui-core-layering",
        summary="kagi-ui-* sources never touch git2 / kagi-git (ADR-0121)",
        # Actual usage only: a path, a `use`, or an `extern crate`. The bare
        # substring also fired on prose such as "libgit2" and forced doc
        # rewrites (#443). Manifests are `ui-core-layering-manifest`.
        pattern=(
            r"\b(git2|kagi_git)::"
            r"|^\s*use\s+(git2|kagi_git)\b"
            r"|extern\s+crate\s+(git2|kagi_git)\b"
        ),
        globs=("crates/kagi-ui-*/**/*.rs",),
        message="kagi-ui-* crates must not depend on git2 or kagi-git (ADR-0121).",
        samples=("use kagi_git::Backend;",),
        samples_ok=("/// libgit2 does this differently; see the git2 token doc.\n",),
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
        samples=("PlanNote::Verbatim(text)",),
        samples_ok=("PlanNote::CheckoutOverlap { files }",),
    ),
    Rule(
        name="modal-lists",
        summary="modal preview lists render every row (#454)",
        # `<list> … .take(` with only chain calls between, so the wrapped form
        # is caught too. Chain segments may carry arguments — `.skip(2)`,
        # `.filter(|f| f.staged)`, `.filter(|f| f.is_new())` — because an
        # empty-paren-only chain let `.iter().filter(…).take(10)` walk past the
        # gate. Row-level truncation (`p.chars().take(80)`) stays allowed: it
        # shortens one line, not the list.
        pattern=(
            r"\.(preview_files|preview_commits|skipped)\s*"
            r"(\.\s*[a-z_]+\((?:[^()]|\([^()]*\))*\)\s*)*"
            r"\.\s*take\("
        ),
        globs=("src/ui/**/*.rs",),
        message=(
            "a modal preview list is truncated with .take(N) — render every row "
            "(list panel + scroll), see #454."
        ),
        samples=(
            "for f in plan\n    .preview_files\n    .iter()\n    .take(10) {}",
            "for f in plan.preview_files.iter().skip(2).take(10) {}",
            "for f in plan.preview_files.iter().filter(|f| f.is_new()).take(10) {}",
        ),
        samples_ok=("let short: String = p.chars().take(80).collect();",),
    ),
    Rule(
        name="shell-hygiene",
        summary="gates and workflows never shell out to grep/find/awk/sed -i",
        # Invocations only (word followed by an argument), so prose and job
        # names are fine. Python gate sources are not scanned: they are `.py`.
        #
        # `[ef]?grep` as one alternative, not `grep` alone: matching the bare
        # word inside `egrep`/`fgrep` put a word character before it, the
        # `[\w./-]` lookbehind rejected the match, and the gate went blind to
        # exactly the two tools whose GNU/BSD split is worst. `git grep` is
        # excluded — it is git's own portable matcher, not the platform's.
        pattern=(
            r"(?<![\w./-])(?<!git )([ef]?grep|find|awk)\s+[-\"'$a-zA-Z0-9]"
            r"|sed\s+-i"
        ),
        globs=("ci/**/*.sh", ".github/workflows/*.yml"),
        message=(
            "CI must not shell out to grep/find/awk/sed -i — the GNU/BSD split "
            "silently no-ops gates (#454). Add a rule in ci/src/kagi_checks/rules.py "
            "and run it with `uv run --project ci check-<name>`."
        ),
        samples=(
            "if grep -rnE 'git2::' src/ui/; then exit 1; fi",
            "egrep -q foo file",
            "fgrep -l bar src/ui/mod.rs",
            "sed -i '' -e s/a/b/ Cargo.toml",
        ),
        samples_ok=(
            "# the old gate used grep -rnE here; now it is a Python rule",
            "git grep -n foo",
        ),
        skip_comments=True,
    ),
    Rule(
        name="uv-invocation",
        summary="workflows run the gates through uv, never a bare interpreter",
        # A bare `python3 ci/…` bypasses the lockfile and the pinned
        # interpreter, which is how "works on my machine" gets back in.
        #
        # `uv run … python -m foo` is the sanctioned form, so a command whose
        # interpreter is already under `uv run` is fine. Scoped per *command*,
        # not per line: the tempered run stops at `;`/`&&`/`|`, so a real
        # `uv run true; python3 ci/gate.py` still fails.
        pattern=(
            r"(?:^|[;&|])[ \t]*(?:(?!uv\s+run)[^;&|\n])*?"
            r"(?<![\w./-])(python3?|pip3?)\s+[-\"'$a-zA-Z0-9]"
        ),
        globs=(".github/workflows/*.yml",),
        message=(
            "run the gates through uv (`uv run --frozen --project ci check-<name>`), "
            "not a bare python/pip — uv pins the interpreter and the lockfile."
        ),
        samples=(
            "run: python3 ci/gate.py check-loc",
            "run: pip install ruff",
            "run: uv run true; python3 ci/gate.py check-loc",
        ),
        samples_ok=(
            "# python3 used to run the gates directly; uv does now",
            "run: uv run --frozen --project ci python -m kagi_checks.cli",
            "run: uv run python -c 'import kagi_checks'",
        ),
        flags=re.MULTILINE,
        skip_comments=True,
    ),
)


# ── Manifest gates: dependency tables, parsed rather than matched ───────────

DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def _dep_tables(data: dict[str, Any]) -> Iterator[dict[str, Any]]:
    """Every dependency table in a parsed manifest, `target.*` ones included."""
    for key in DEP_TABLES:
        table = data.get(key)
        if isinstance(table, dict):
            yield table
    targets = data.get("target")
    if isinstance(targets, dict):
        for cfg in targets.values():
            if isinstance(cfg, dict):
                yield from _dep_tables(cfg)


def crate_name(name: str) -> str:
    """Cargo accepts either spelling of a crate name; normalise to hyphens."""
    return name.replace("_", "-")


def workspace_dep_aliases() -> dict[str, str]:
    """`[workspace.dependencies]` alias -> effective crate name, from the root manifest.

    Round-2 review: a `{ workspace = true }` dependency carries no `package`
    of its own, so a rename declared once in the root
    (`backend = { package = "kagi-git" }`) made every inheriting crate look
    like it depended on `backend` — a clean bypass of both layering gates.
    """
    root_manifest = ROOT / "Cargo.toml"
    if not root_manifest.is_file():
        return {}
    try:
        data = tomllib.loads(root_manifest.read_text(encoding="utf-8", errors="replace"))
    except tomllib.TOMLDecodeError:
        return {}
    workspace = data.get("workspace")
    if not isinstance(workspace, dict):
        return {}
    deps = workspace.get("dependencies")
    if not isinstance(deps, dict):
        return {}
    out: dict[str, str] = {}
    for key, spec in deps.items():
        name = key
        if isinstance(spec, dict):
            package = spec.get("package")
            if isinstance(package, str):
                name = package
        out[key] = name
    return out


def manifest_dep_names(text: str, aliases: dict[str, str] | None = None) -> list[tuple[str, str]]:
    """(declared key, effective crate name) for every dependency in a manifest.

    The effective name is the dependency's `package` rename when it has one,
    which is the whole point: `backend = { package = "kagi-git", path = … }` is
    a kagi-git dependency, and only the parsed manifest says so. A
    `{ workspace = true }` dependency inherits its rename from the root
    `[workspace.dependencies]` table, so that map is consulted too.
    """
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return []
    inherited = workspace_dep_aliases() if aliases is None else aliases
    out: list[tuple[str, str]] = []
    for table in _dep_tables(data):
        for key, spec in table.items():
            name = key
            if isinstance(spec, dict):
                package = spec.get("package")
                if isinstance(package, str):
                    name = package
                elif spec.get("workspace") is True:
                    name = inherited.get(key, key)
            out.append((key, name))
    return out


def manifest_dep_hits(
    text: str,
    banned: tuple[str, ...],
    aliases: dict[str, str] | None = None,
) -> list[tuple[str, str]]:
    """(declared key, crate name) for each dependency resolving to a banned crate."""
    wanted = {crate_name(name) for name in banned}
    return [
        (key, name) for key, name in manifest_dep_names(text, aliases) if crate_name(name) in wanted
    ]


@dataclass(frozen=True)
class ManifestRule:
    """A "this crate must not be a dependency" gate over Cargo manifests."""

    name: str
    summary: str
    globs: tuple[str, ...]
    banned: tuple[str, ...]
    message: str
    samples: tuple[str, ...] = ()
    samples_ok: tuple[str, ...] = ()
    excludes: tuple[str, ...] = ()

    def fires_on(self, text: str) -> bool:
        return bool(manifest_dep_hits(text, self.banned))

    def hits(self) -> list[tuple[Path, str, str]]:
        """(file, declared key, banned crate name) for every offending dependency."""
        out: list[tuple[Path, str, str]] = []
        for rel in iter_files(list(self.globs), self.excludes):
            for key, name in manifest_dep_hits(read_text(rel), self.banned):
                out.append((rel, key, name))
        return out


MANIFEST_RULES: tuple[ManifestRule, ...] = (
    ManifestRule(
        name="ui-core-layering-manifest",
        summary="kagi-ui-* manifests never depend on git2 / kagi-git (ADR-0121)",
        globs=("crates/kagi-ui-*/**/Cargo.toml",),
        banned=("git2", "kagi-git"),
        message="kagi-ui-* crates must not depend on git2 or kagi-git (ADR-0121).",
        samples=(
            '[dependencies]\nbackend = { package = "kagi-git", path = "../kagi-git" }\n',
            '[dependencies.kagi-git]\npath = "../kagi-git"\n',
            '[target.\'cfg(unix)\'.dev-dependencies]\n"git2" = "0.19"\n',
        ),
        samples_ok=('[dependencies]\nkagi-ui-core = { path = "../kagi-ui-core" }\n',),
    ),
    ManifestRule(
        name="mcp-gpui-manifest",
        summary="crates/kagi-mcp manifests never depend on gpui (ADR-0163 / #331)",
        globs=("crates/kagi-mcp/**/Cargo.toml",),
        banned=("gpui",),
        message="crates/kagi-mcp must not depend on gpui (ADR-0163 / #331).",
        samples=('[dependencies]\nui = { package = "gpui", git = "https://github.com/zed" }\n',),
        samples_ok=('[dependencies]\ngpui-component = { git = "https://example" }\n',),
    ),
)


# ── Ratchets: per-file counts that may shrink but never grow ─────────────────

# `\s*`, so the rustfmt-wrapped call counts too; the same-line form has zero
# tolerance in the `klog-raw` rule above.
KLOG_RAW = re.compile(r'(?:eprintln|println)!\(\s*"\[kagi\]')


def klog_count(text: str) -> int:
    """Raw `[kagi]` emissions in one file (the klog! single-channel ratchet)."""
    return len(KLOG_RAW.findall(text))


def loc_count(text: str) -> int:
    """Line count of one file, or 0 while it is inside the LOC ceiling."""
    lines = len(text.splitlines())
    return lines if lines > LOC_CEILING else 0


@dataclass(frozen=True)
class Ratchet:
    """A per-file count that may shrink but never grow.

    `count` takes the file's text rather than reading it, so the selftest can
    hand it a sample: an unguarded counter that matches nothing zeroes every
    file, prints `::notice … shrank`, and never fails — the silent-green
    failure this whole project exists to prevent.
    """

    name: str
    summary: str
    baseline: str
    unit: str
    guidance: str
    globs: tuple[str, ...]
    count: Callable[[str], int]
    # (text, count the counter must produce) — a 0 pins a carve-out.
    samples: tuple[tuple[str, int], ...] = ()
    excludes: tuple[str, ...] = ()

    def counts(self) -> dict[str, int]:
        out: dict[str, int] = {}
        for rel in iter_files(list(self.globs), self.excludes):
            found = self.count(read_text(rel))
            if found:
                out[rel.as_posix()] = found
        return out


RATCHETS: tuple[Ratchet, ...] = (
    Ratchet(
        name="klog",
        summary="no new raw [kagi] lines outside klog! (ratchet, #396)",
        baseline="ci/klog-baseline.txt",
        unit='raw (e)println!("[kagi]…") call(s)',
        guidance="[kagi] contract lines must go through klog! (ADR-0096 / #396).",
        globs=RUST_SOURCES,
        count=klog_count,
        samples=(
            ('eprintln!(\n    "[kagi] watcher: {}",\n    e\n);\n', 1),
            ('eprintln!("[kagi] watcher: {}", e);\n', 1),
            ('klog!("watcher: {}", e);\n', 0),
        ),
        excludes=("crates/kagi-ui-core/src/klog.rs",),
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
        globs=("src/**/*.rs", "crates/*/src/**/*.rs"),
        count=loc_count,
        samples=(
            ("fn f() {}\n" * (LOC_CEILING + 1), LOC_CEILING + 1),
            ("fn f() {}\n" * LOC_CEILING, 0),
        ),
        excludes=("/tests/",),
    ),
)


# ── Custom checks: rules a single regex cannot express ──────────────────────


def ui_lateral_crates() -> list[Path]:
    return sorted(d for d in (ROOT / "crates").glob("kagi-ui-*") if d.is_dir())


def ui_lateral_hits() -> list[tuple[Path, int, str, str]]:
    """Feature crates may import kagi-ui-core, never a sibling kagi-ui-* crate.

    The allowed set depends on which crate is being scanned, which is why this
    is a function and not a `Rule`. `kagi-ui-core` is the shared base, and a
    crate naming itself is not an import (ADR-0121).
    """
    # Actual usage only — a path, a `use`, or an `extern crate`. Matching the
    # bare crate name also flagged doc comments that merely *mention* a sibling
    # ("moved here from `kagi-ui-file-history`"), the same false-positive class
    # #443 fixed for git2.
    rust = re.compile(
        r"\b(kagi_ui_[a-z_]+)::"
        r"|^\s*use\s+(kagi_ui_[a-z_]+)\b"
        r"|extern\s+crate\s+(kagi_ui_[a-z_]+)\b",
        re.MULTILINE,
    )
    out: list[tuple[Path, int, str, str]] = []
    for crate in ui_lateral_crates():
        hyphen = crate.name
        allowed = {"kagi-ui-core", hyphen}
        for rel in iter_files([f"crates/{hyphen}/**/*.rs"]):
            text = read_text(rel)
            lines = text.splitlines()
            for match in rust.finditer(text):
                name = next(g for g in match.groups() if g)
                if crate_name(name) in allowed:
                    continue
                line_no = text.count("\n", 0, match.start()) + 1
                out.append((rel, line_no, lines[line_no - 1].strip(), hyphen))
    return out


def ui_lateral_manifest_hits() -> list[tuple[Path, str, str, str]]:
    """(manifest, declared key, sibling crate, scanned crate) for lateral deps.

    Parsed, not matched, for the reason `manifest_dep_names` exists: a rename
    (`sib = { package = "kagi-ui-editor" }`) is a lateral dependency that no
    dependency-line pattern can see.
    """
    out: list[tuple[Path, str, str, str]] = []
    for crate in ui_lateral_crates():
        hyphen = crate.name
        allowed = {"kagi-ui-core", hyphen}
        for rel in iter_files([f"crates/{hyphen}/**/Cargo.toml"]):
            for key, name in manifest_dep_names(read_text(rel)):
                norm = crate_name(name)
                if not norm.startswith("kagi-ui-") or norm in allowed:
                    continue
                out.append((rel, key, norm, hyphen))
    return out


def ui_lateral_crate_count() -> int:
    return len(ui_lateral_crates())
