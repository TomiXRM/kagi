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
from collections.abc import Callable, Iterable, Iterator
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

ROOT = Path(__file__).resolve().parents[3]

# Never source: build output, vendored code, tool caches.
SKIP_PARTS = frozenset({".git", "target", "vendor", "node_modules", ".claude", ".venv"})

LOC_CEILING = 800


def is_excluded(rel: Path, excludes: tuple[str, ...]) -> bool:
    """Whether a repo-relative path belongs to an excluded root directory."""
    return any(rel.as_posix().startswith(exclude) for exclude in excludes)


def iter_files(
    globs: list[str],
    excludes: tuple[str, ...] = (),
    root: Path = ROOT,
) -> list[Path]:
    """Root-relative files matching any glob, minus build output and `excludes`.

    Replaces `find … -not -path …`: one traversal, identical on every OS.

    `root` defaults to the repository, which is what every gate scans. It is a
    parameter so a selftest can point the *same* traversal at a fixture tree
    (`ui_lateral_selftest`): a second walker written for the test would prove
    the test's walker works, not the gate's.
    """
    seen: dict[Path, None] = {}
    for pattern in globs:
        for path in sorted(root.glob(pattern)):
            if not path.is_file():
                continue
            rel = path.relative_to(root)
            if SKIP_PARTS & set(rel.parts):
                continue
            if is_excluded(rel, excludes):
                continue
            seen[rel] = None
    return list(seen)


def read_text(rel: Path, root: Path = ROOT) -> str:
    return (root / rel).read_text(encoding="utf-8", errors="replace")


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
    # (repo-relative path, should be excluded), checked by the selftest.
    path_samples: tuple[tuple[str, bool], ...] = ()
    flags: int = re.MULTILINE | re.DOTALL
    # Skip matches on `#`-comment lines. Set for rules whose subject is shell
    # or YAML, where the words being banned also appear in prose explaining
    # why they are banned (this file's own workflow comment did exactly that).
    skip_comments: bool = False
    # A source selector may mask non-candidates while preserving offsets, so
    # diagnostics and selftest still use the same pattern and original lines.
    select_source: Callable[[str], str] | None = None

    def compiled(self) -> re.Pattern[str]:
        return re.compile(self.pattern, self.flags)

    def _matches(self, text: str) -> list[tuple[int, str]]:
        """Line number + text of every real (non-skipped) match in `text`."""
        rx = self.compiled()
        lines = text.splitlines()
        out: list[tuple[int, str]] = []
        source = self.select_source(text) if self.select_source is not None else text
        for match in rx.finditer(source):
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


def _rust_literal_spans(text: str) -> tuple[str, list[tuple[int, int]]]:
    """Mask strings/comments for structural scanning; retain string spans.

    Rust block comments nest; raw strings can contain quotes, comment markers
    and braces. Neither may accidentally create a locale arm or end its body.
    Character literals are masked too, but lifetimes such as `'static` are not.
    """
    code = list(text)
    literals: list[tuple[int, int]] = []
    token = re.compile(r'//|/\*|\br(#+)?"|"(?:\\[\s\S]|[^"\\])*"|\'(?:\\.|[^\'\\])\'')
    comment = re.compile(r"/\*|\*/")
    cursor = 0
    while match := token.search(text, cursor):
        start, end = match.span()
        value = match.group()
        if value == "//":
            end = text.find("\n", end)
            if end < 0:
                end = len(text)
        elif value == "/*":
            depth = 1
            while depth and (marker := comment.search(text, end)):
                depth += 1 if marker.group() == "/*" else -1
                end = marker.end()
            if depth:
                end = len(text)
        elif value.startswith("r"):
            closing = '"' + (match.group(1) or "")
            close = text.find(closing, end)
            end = len(text) if close < 0 else close + len(closing)
            literals.append((start, end))
        elif value.startswith('"'):
            literals.append((start, end))
        code[start:end] = ["\n" if c == "\n" else " " for c in text[start:end]]
        cursor = end
    return "".join(code), literals


def _rust_expression_end(code: str, start: int, *, delimited: bool = False) -> int:
    """End an arm/initializer without crossing its balanced delimiters."""
    stack: list[str] = []
    pairs = {"(": ")", "[": "]", "{": "}"}
    first = start
    while first < len(code) and code[first].isspace():
        first += 1
    for pos in range(start, len(code)):
        char = code[pos]
        if char in pairs:
            stack.append(pairs[char])
        elif char in ")]}":
            if not stack:
                return pos
            if char == stack[-1]:
                stack.pop()
                # Blocks may omit the arm comma; tuple patterns end here too
                # when the caller explicitly requests a delimited region.
                if not stack and (delimited or code[first : first + 1] == "{"):
                    return pos + 1
        elif char in ",;" and not stack:
            return pos
    return len(code)


def _translation_tuple_members(
    code: str, literals: list[tuple[int, int]], start: int, end: int, arity: int, locale: str
) -> list[tuple[int, int]]:
    """Select EN/JA members of an explicitly bilingual tuple owner."""
    regions: list[tuple[int, int]] = []
    cursor = start
    while (opening := code.find("(", cursor, end)) >= 0:
        closing = _rust_expression_end(code, opening, delimited=True)
        if closing > end:
            break
        # Consume the whole tuple so nested tuples/calls in its EN member
        # cannot become independent JA candidates.
        cursor = closing
        members: list[tuple[int, int]] = []
        member_start = opening + 1
        depth = 0
        for pos in range(member_start, closing - 1):
            char = code[pos]
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
            elif char == "," and depth == 0:
                members.append((member_start, pos))
                member_start = pos + 1
        tail_end = closing - 1
        # String contents were masked; a whitespace-only member can still
        # contain a literal. An actual trailing comma contributes no member.
        if code[member_start:tail_end].strip() or any(
            member_start <= left < tail_end for left, _ in literals
        ):
            members.append((member_start, tail_end))
        if len(members) == arity:
            regions.append(members[-1 if locale == "Ja" else -2])
    return regions


def _translation_literals(text: str, locale: str, *, display_only: bool = False) -> str:
    """Select explicit translation owners, excluding comments and test data.

    Locale arms, bilingual label tuples and named language renderers share the
    same structural scanner so raw strings cannot manufacture translation arms.
    """
    code, literals = _rust_literal_spans(text)
    regions: list[tuple[int, int]] = []
    ignored: list[tuple[int, int]] = []
    arrow = re.compile(r"\s*(?:if\b[^;]*?)?=>")
    other_locale = "En" if locale == "Ja" else "Ja"
    for language, destinations in ((locale, regions), (other_locale, ignored)):
        locale_token = re.compile(rf"\b(?:Lang\s*::\s*)?{language}\b")
        arms = re.compile(rf"\(|{locale_token.pattern}")
        for arm in arms.finditer(code):
            pattern_end = arm.end()
            if code[arm.start()] == "(":
                pattern_end = _rust_expression_end(code, arm.start(), delimited=True)
                if not locale_token.search(code, arm.start(), pattern_end):
                    continue
            if body := arrow.match(code, pattern_end):
                destinations.append((body.end(), _rust_expression_end(code, body.end())))
    constant_names = (
        r"(?:ADVICE_(?!EN_|\w*_EN\b)\w+|JA_\w+|\w+_JA)" if locale == "Ja" else r"(?:EN_\w+|\w+_EN)"
    )
    # Domain message_en/advice_template_en feed persisted receipts. The UI
    # jargon gate selects their display-only counterparts, never stored prose.
    suffix = "display_en" if display_only and locale == "En" else locale.lower()
    owners = re.compile(
        rf"\bfn\s+\w+_{suffix}(?:_\w+)?\s*\([^;{{}}]*\)[^;{{}}]*\{{"
        rf"|\bconst\s+{constant_names}\s*:[^;=]*="
        rf"|\bmacro_rules!\s+\w+_{suffix}\s*\{{"
    )
    for owner in owners.finditer(code):
        start = owner.end() - 1 if owner.group().endswith("{") else owner.end()
        regions.append((start, _rust_expression_end(code, start)))
    # A two-language helper may return EN before its JA-only body.
    early_en = re.compile(
        r"\bfn\s+\w+\s*\([^;{}]*\b(?P<locale>\w+)\s*:\s*Lang\b[^;{}]*\)"
        r"[^;{}]*\{\s*if\s+(?P=locale)\s*==\s*Lang\s*::\s*En\s*\{"
        r"\s*return\b[^;{}]*;\s*\}"
    )
    for owner in early_en.finditer(code):
        body_start = code.index("{", owner.start())
        destination = regions if locale == "Ja" else ignored
        destination.append((owner.end(), _rust_expression_end(code, body_start)))
        if locale == "En":
            regions.append((body_start, owner.end()))
    # Label tables contain (EN, JA) or (internal tag, EN, JA). Internal tags
    # are never translation candidates.
    table_owners = re.compile(
        r"\bconst\s+(?:LABELS|TRANSLATIONS|\w+_(?:LABELS|TRANSLATIONS))\s*:"
        r"\s*&\s*\[\s*\((?P<types>[^()]*)\)\s*\]\s*=\s*&\s*\["
    )
    for table in table_owners.finditer(code):
        types = [item.strip() for item in table.group("types").split(",") if item.strip()]
        if len(types) not in (2, 3) or not all(
            re.fullmatch(r"&(?:'\w+\s+)?str", item) for item in types
        ):
            continue
        start = table.end() - 1
        end = _rust_expression_end(code, start, delimited=True)
        regions.extend(_translation_tuple_members(code, literals, start, end, len(types), locale))
    pair_owners = re.compile(
        r"\bfn\s+(?:label|\w+_label)\s*\([^;{}]*\)\s*->\s*\("
        r"\s*&(?:'\w+\s+)?str\s*,\s*&(?:'\w+\s+)?str\s*,?\s*\)\s*\{"
    )
    for pair in pair_owners.finditer(code):
        start = pair.end() - 1
        regions.extend(
            _translation_tuple_members(
                code, literals, start, _rust_expression_end(code, start), 2, locale
            )
        )
    # Test fixtures and assertions may deliberately quote the banned words.
    # Mask only test bodies, never the containing production file.
    test_bodies = re.compile(r"#\[\s*(?:test|cfg\s*\(\s*test\s*\))\s*\][^{;]*\{")
    for test in test_bodies.finditer(code):
        start = test.end() - 1
        ignored.append((start, _rust_expression_end(code, start)))
    if display_only:
        # Explicit contract helpers retain logging/persistence wording even
        # when their implementation has locale arms.
        contracts = re.compile(r"\bfn\s+\w+_contract\s*\([^;{}]*\)[^;{}]*\{")
        for contract in contracts.finditer(code):
            start = contract.end() - 1
            ignored.append((start, _rust_expression_end(code, start)))
        # Match inputs identify legacy messages; only their replacement
        # expressions are displayed. Strings/comments are already masked.
        for start, end in literals:
            if re.match(r"\s*(?:\|\s*)*=>", code[end:]):
                ignored.append((start, end))
    # Contract logging is not UI copy, even inside a translation owner.
    for log in re.finditer(r"\bklog\s*!\s*[(\[{]", code):
        start = log.end() - 1
        ignored.append((start, _rust_expression_end(code, start, delimited=True)))
    selected = ["\n" if c == "\n" else " " for c in text]
    for start, end in literals:
        if not any(left <= start < right for left, right in regions):
            continue
        if any(left <= start < right for left, right in ignored):
            continue
        literal = text[start:end]
        if locale == "Ja":
            # Only the name-collision occurrence is exempt, not every conflict
            # in a string that happens to mention refs or a namespace.
            literal = re.sub(
                r"(?:名前空間|ref\s+namespace)\s*(?:が|の|で|と)?\s*競合",
                lambda match: match.group().replace("競合", "  "),
                literal,
            )
        selected[start:end] = literal
    return "".join(selected)


def _ja_git_literals(text: str) -> str:
    return _translation_literals(text, "Ja")


def _ui_display_literals(text: str) -> str:
    en = _translation_literals(text, "En", display_only=True)
    ja = _translation_literals(text, "Ja", display_only=True)
    return "".join(left if left != " " else right for left, right in zip(en, ja, strict=True))


JA_GIT_TERMS = (
    "退避",
    "作業ツリー",
    "コミット",
    "コミッター",
    "サブモジュール",
    "ブランチ",
    "リモート",
    "マージ",
    "取り込み",
    "ステージ",
    "チェックアウト",
    "フェッチ",
    "プッシュ",
    "タグ",
    "リベース",
    "チェリーピック",
    "リバート",
    "アメンド",
    "スカッシュ",
    "インデックス",
    "ヘッド",
    "アップストリーム",
    "衝突",
    "コンフリクト",
    "競合",
)


RULES: tuple[Rule, ...] = (
    Rule(
        name="ui-internal-jargon",
        summary="EN/JA UI copy describes user actions, not implementation details (#1588)",
        pattern=(
            r"(?<![A-Za-z0-9_])ADR-\d"
            r"|(?<![A-Za-z0-9_])MVP(?![A-Za-z0-9_])|\[kagi\]"
            r"|(?i:(?<![A-Za-z0-9_])preflight(?![A-Za-z0-9_])"
            r"|(?<![A-Za-z0-9_])session\s+unavailable(?![A-Za-z0-9_]))"
            r"|(?<![A-Za-z0-9_])[A-Za-z_][A-Za-z0-9_]*"
            r"(?:::[A-Za-z_][A-Za-z0-9_]*)*\s*\(\s*\)"
        ),
        globs=(
            "crates/kagi-ui-core/src/i18n/**/*.rs",
            "crates/kagi-domain/src/plan_note/**/*.rs",
        ),
        select_source=_ui_display_literals,
        message=(
            "User-facing EN/JA must describe the action or next step, not ADRs, "
            "development scope, log prefixes, pipeline stages, sessions or Rust calls (#1588)."
        ),
        samples=(
            *(
                sample
                for phrase in (
                    "See ADR-0024",
                    "Outside MVP scope",
                    "[kagi] operation failed",
                    "Preflight failed",
                    "repository session unavailable",
                    "Use stage_file() first",
                    "Call Repository::open()",
                    "Call refresh()",
                )
                for sample in (
                    f'(En, Action) => "{phrase}",',
                    f'Lang::En => format!(r##"{phrase}: {{err}}"##),',
                    f'Lang::Ja => "{phrase}",',
                    f'fn message_display_en(&self) -> String {{ "{phrase}".into() }}',
                    f'macro_rules! advice_template_display_en {{ (Action) => {{ "{phrase}" }}; }}',
                )
            ),
            'const ACTION_EN: &str = "MVP";',
            'const EN_ACTION: &str = "ADR-1";',
            'const LABELS: &[(&str, &str, &str)] = &[("tag", "Preflight", "確認")];',
            'const TRANSLATIONS: &[(&str, &str)] = &[("MVP", "確認")];',
            "fn label(self) -> (&'static str, &'static str) { "
            'match self { Action => ("session unavailable", "確認"), } }',
            "fn heading(language: Lang) -> &str { "
            'if language == Lang::En { return "MVP"; } "確認" }',
            '/* outer /* nested */ Lang::Ja => "safe" */ Lang::En => "MVP",',
            '#[cfg(test)] mod tests { const ACTION_EN: &str = "safe"; }\nLang::En => "preflight",',
            'const ADVICE_ACTION: &str = "MVP";',
            'fn message_ja(&self) -> String { "ADR-0024".into() }',
            'const LABELS: &[(&str, &str, &str)] = &[("tag", "Checking", "MVP")];',
            'const TRANSLATIONS: &[(&str, &str)] = &[("Checking", "preflight")];',
            "fn label(self) -> (&'static str, &'static str) { "
            'match self { Action => ("Checking", "MVP"), } }',
            "fn heading(language: Lang) -> &str { "
            'if language == Lang::En { return "Checking"; } "MVP" }',
            'Lang::Ja => "MVP対象外です。ADR-0024を参照",',
            'Lang::Ja => "preflightに失敗しました",',
            'Lang::Ja => "先にstage_file()を呼んでください",',
            "fn op_failed_contract() -> String { match lang() { "
            'Lang::En => "Preflight failed", Lang::Ja => "preflight失敗", } }\n'
            "fn op_failed() -> String { match lang() { "
            'Lang::En => "MVP", Lang::Ja => "確認", } }',
            "fn message_display_en(&self) -> String { match self { "
            '"safe" => "MVP", _ => "Ready" } }',
        ),
        samples_ok=(
            *(
                sample
                for phrase in (
                    "ADR-0024",
                    "MVP",
                    "[kagi]",
                    "preflight",
                    "session unavailable",
                    "stage_file()",
                    "Repository::open()",
                    "refresh()",
                )
                for sample in (
                    f'// Lang::En => "{phrase}",\nLang::En => "Try again",',
                    f'/* outer /* nested */ Lang::En => "{phrase}", */',
                    f'Lang::En => {{ /* {phrase} */ "Try again" }},',
                    f'fn message_en(&self) -> String {{ "{phrase}".into() }}',
                    f'macro_rules! advice_template_en {{ (Action) => {{ "{phrase}" }}; }}',
                    f'// Lang::Ja => "{phrase}",\nLang::Ja => "確認",',
                    f'#[cfg(test)] mod tests {{ Lang::Ja => "{phrase}", }}',
                    f'Lang::Ja => {{ klog!("{phrase}"); "確認" }},',
                    f'#[cfg(test)] mod tests {{ Lang::En => "{phrase}", }}',
                    f'#[test] fn sample() {{ assert_eq!(render(), "{phrase}"); }}',
                    f'Lang::En => {{ klog!("{phrase}"); "Try again" }},',
                    f"fn op_failed_contract() -> String {{ match lang() {{ "
                    f'Lang::En => "{phrase}", Lang::Ja => "{phrase}", }} }}',
                    f"fn message_display_en(&self) -> String {{ match self {{ "
                    f'"{phrase}" => "Ready", _ => "Try again" }} }}',
                    f'Lang::En => match legacy {{ "{phrase}" => "Ready", _ => "Try again" }},',
                )
            ),
            "enum Internal { MVP, Preflight, SessionUnavailable }\nLang::En => stage_file(),",
            'const INTERNAL_TAG: &str = "[kagi]";',
            'const LABELS: &[(&str, &str, &str)] = &[("[kagi]", "Checking", "確認")];',
            'const TRANSLATIONS: &[(&str, &str)] = &[("Checking", "確認")];',
            "fn label(self) -> (&'static str, &'static str) { "
            'match self { Preflight => ("Checking", "確認"), } }',
            "fn heading(language: Lang) -> &str { "
            'if language == Lang::En { return "Checking"; } "確認" }',
            'let quoted_source = r#"Lang::En => "MVP""#;',
            'Lang::En => "Stage changes, then commit. Check Git status and try again.",',
            'Lang::En => "ADR notes, MVPlayer, preflighted, session is unavailable, file(s)",',
        ),
    ),
    Rule(
        name="ja-git-terms",
        summary="JA localization keeps Git domain terms in English (ADR-0048 / #1465)",
        pattern="|".join(
            {
                # Owner-approved feature name (#1816), not the Git commit verb.
                "コミット": r"(?<!スマート)コミット",
                "マージ": r"マージ(?!ン)",
                "ステージ": r"ステージ(?!マネージャ)",
                "リモート": r"リモート(?!デスクトップ)",
            }.get(term, re.escape(term))
            for term in JA_GIT_TERMS
        )
        + r"|(?<![ァ-ヶー])プル(?!ダウン)",
        globs=("src/**/*.rs", "crates/**/src/**/*.rs"),
        select_source=_ja_git_literals,
        message=(
            "JA Git domain terms must stay English (ADR-0048 / #1465); "
            "keep Japanese grammar, not translated Git vocabulary."
        ),
        samples=(
            *(
                sample
                for term in (*JA_GIT_TERMS, "プル", "プルリクエスト")
                for sample in (
                    f'(Ja, GitAction) => "{term} する",',
                    f'(Lang::Ja,\n GitAction) => format!(\n "{term}: {{name}}"\n),',
                    f'pub(crate) const ADVICE_ACTION: &str =\n "{term} してください";',
                )
            ),
            'Lang::Ja => {\n let label = "commit";\n format!("{} をコミット", label)\n}',
            '(Ja, Action) => "スマートコミット後にコミットする",',
            'Lang::Ja => match state { Some(_) => "ブランチ", None => "なし" },',
            'fn note_ja(note: &Note) -> String {\n format!("未コミット: {}", note)\n}',
            'fn note_ja_any(note: &Note) -> String { format!("作業ツリー: {}", note) }',
            'const ACTION_JA: &str = r##"リモート "origin" の\nブランチ"##;',
            'const JA_ACTION: &str = concat!("未",\n "コミット");',
            '(Ja, Action { state: Some((left, right)) }) => "コミット",',
            '(HealthFinding::Missing, Lang::Ja) => "コミット",',
            '(Action { state: Some((left, right)) }, Ja) => "ブランチ",',
            '(Ja, Action) if count > 0 => "ブランチ",',
            'const ADVICE_ACTION: &str = "コミット";',
            'Lang::Ja /* wrapped */ =>\n format!("コミット\\n\\\nしてください"),',
            'Lang::Ja => "名前空間が競合。ファイルの競合を解決してください",',
            'Lang::Ja => "ref を変更する前に競合を解決してください",',
            'Lang::Ja => { "commit" }\n Lang::En => "コミット",\nLang::Ja => "ブランチ",',
            'Lang::Ja => format!("{}", Msg::Title.t_for(Lang::En)),\nLang::Ja => "コミット",',
            '/* outer /* nested */ Lang::En => "safe" */ Lang::Ja => "コミット",',
            '#[cfg(test)] mod tests { const ADVICE_ACTION: &str = "コミット"; }\n'
            'const ADVICE_ACTION: &str = "ブランチ";',
            'const LABELS: &[(&str, &str, &str)] = &[("merge", "Merging", "マージ")];',
            "const LABELS: &[(&str, &str, &str)] = &[\n"
            '("merge",\n "Merging",\n r#"競合を解決"#,\n),\n];',
            'const ACTION_LABELS: &[(&str, &str)] = &[("Commit", "コミット")];',
            'const TRANSLATIONS: &[(&str, &str)] = &[("Branch",\n "ブランチ",)];',
            "fn label(self) -> (&'static str, &'static str) {\n"
            'match self { Commit => ("Commit", "コミット"), }\n}',
            "fn action_label(self) -> (&'static str, &'static str) {\n"
            'match self { Merge => (\n "Merge",\n r##"競合 "file""##,\n), }\n}',
            "const LABELS: &[(&str, &str, &str)] = &[\n"
            '("commit", /* EN */ "Commit", /* JA */ "コミット"),\n];',
            *(
                "fn menu_heading(section: &str, language: Lang) -> &str {\n"
                "if language == Lang::En { return section; }\n"
                f'match section {{ "Git" => "{term}", _ => section }}\n}}'
                for term in ("ブランチ", "コミット")
            ),
        ),
        samples_ok=(
            "fn menu_heading(section: &str, language: Lang) -> &str {\n"
            'if language == Lang::En { return "Quoted コミット"; }\n'
            'match section { "Branch" => "Branch", "File" => "ファイル", _ => section }\n}',
            '(Ja, Action) => "stash / working tree / commit / branch / conflict",',
            'Lang::En => "User branch name: コミット",',
            '(En,\n Action) => format!(\n "User message: マージ\\nブランチ"\n),',
            '(HealthFinding::Missing, Lang::En) => "Quoted コミット",',
            'const ADVICE_ACTION_EN: &str = "Quoted コミット";',
            'const USER_BRANCH_NAME: &str = "ブランチ";',
            'let user_message = "マージ"; render(user_message);',
            'Lang::Ja => format!("{user_message}: {branch}"),',
            '// (Ja, Action) => "コミット",\nLang::Ja => "commit",',
            '/* Lang::Ja => "ブランチ" */ Lang::Ja => "branch",',
            '/* outer /* nested */ const ADVICE_ACTION: &str = "マージ"; */',
            'Lang::Ja => {\n // コミット\n /* ブランチ */ "commit"\n}',
            'Lang::Ja => r##"comment syntax /* and */ is data, not code"##,',
            'let quoted_source = r#"Lang::Ja => "コミット""#;',
            'Lang::Ja => "パープル",',
            'Lang::Ja => "サンプル",',
            'Lang::Ja => "マージン",',
            'Lang::Ja => "ステージマネージャ",',
            'Lang::Ja => "プルダウン",',
            'Lang::Ja => "リモートデスクトップ",',
            '(Ja, SettingsPaneSmartCommit) => "スマートコミット",',
            'Lang::Ja => "既存の ref `{}` と名前空間が競合しています。",',
            'fn name_error_ja() -> String { "ref namespace が競合しています".into() }',
            'const ADVICE_NAME: &str = "ref の名前空間が競合しています";',
            'assert!(!Msg::Merge.t_for(Lang::Ja).contains("マージ"));',
            "assert!(!m.t().contains('衝'), \"{:?} still contains 衝突\", m);",
            'assert_eq!(render(Lang::Ja, "コミット"), "commit");',
            'Lang::En => format!("{}", Msg::Title.t_for(Lang::Ja)),\n'
            '(En, Action) => "Quoted コミット",',
            'const ADVICE_EN_ACTION: &str = "Quoted コミット";',
            '// Lang::Ja => "競合"\n(En, Action) => r##"Quoted リモート "origin" のブランチ"##,',
            'Lang::Ja => { "commit" }\nLang::En => "コミット",',
            '(Ja, Action) => format!("commit: {value}"), (En, Action) => "コミット",',
            'const ADVICE_ACTION: &str = "commit";\nfn note_en() -> String { "コミット".into() }',
            "fn note_ja() -> String { match lang() {\n"
            'Lang::En => "コミット".into(), Lang::Ja => "commit".into(),\n} }',
            "#[test]\n fn terms_ja_are_english() {\n"
            'assert!(!m.t().contains("コミット"));\n'
            'let expected = match lang() { Ja => "マージ", En => "merge" };\n}',
            '#[cfg(test)] mod tests { const ADVICE_ACTION: &str = "ブランチ"; }',
            "const LABELS: &[(&str, &str, &str)] = &[\n"
            '("コミット", "Quoted マージ", "commit"),\n];',
            "const LABELS: &[(&str, &str, &str)] = &[\n"
            '("colour", "Purple", "パープル"),\n'
            '("name", "Name", "ref の名前空間が競合しています"),\n];',
            'const ACTION_LABELS: &[(&str, &str)] = &[("Quoted コミット", "commit")];',
            'const TRANSLATIONS: &[(&str, &str)] = &[("Quoted ブランチ", "branch")];',
            "fn label(self) -> (&'static str, &'static str) {\n"
            'match self { Commit => (r##"User wrote "コミット""##, "commit"), }\n}',
            "fn label(self) -> (&'static str, &'static str) {\n"
            'match self { Commit => (choose(("コミット", "ブランチ")), "commit"), }\n}',
            'const USER_DATA: &[(&str, &str, &str)] = &[("a", "b", "コミット")];',
            'const LABELS: &[(&str, Msg)] = &[("コミット", Msg::Commit)];',
            '// const LABELS: &[(&str, &str, &str)] = &[("a", "b", "コミット")];',
            "#[cfg(test)] mod tests {\n"
            'const LABELS: &[(&str, &str, &str)] = &[("merge", "Merge", "マージ")];\n}',
        ),
    ),
    Rule(
        name="app-layering",
        summary="application layer has no UI or direct I/O dependencies (#484)",
        pattern=r"\b(?:gpui|git2|settings|i18n)\s*::|\bstd\s*::\s*(?:fs|process|net)\b",
        globs=("src/app/**/*.rs",),
        message="src/app must use typed backend capabilities, not UI or direct I/O",
        samples=(
            "gpui::Context",
            "std::fs::read(path)",
            "std::process::Command",
            "settings::load()",
            "git2::Repository",
            "i18n::Msg",
        ),
        samples_ok=("Backend::open(path)",),
    ),
    Rule(
        name="fault-test-only",
        summary="remove fault injection is called only from tests (#484 N1)",
        pattern=r"(?<!fn )\bwith_fault_for_test\b",
        globs=("**/*.rs",),
        excludes=("tests/",),
        message="with_fault_for_test callers must live under tests/",
        samples=(
            "job.with_fault_for_test(point)",
            "RemoveJob::with_fault_for_test(job, point)",
            "let f = RemoveJob::with_fault_for_test;",
        ),
        samples_ok=("pub fn with_fault_for_test(mut self, point: Fault) -> Self { self }",),
        path_samples=(("tests/app_remove_test.rs", True), ("src/contests/x.rs", False)),
    ),
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
        name="modal-slot-storage",
        summary="modal replacement stays behind typed modal_state transitions (#718)",
        pattern=(
            r"(?:\b[A-Za-z_]\w*\.active_modal\s*(?:=|\.replace\s*\(|\.take\s*\())"
            r"|(?:&mut\s+[A-Za-z_]\w*\.active_modal\b)"
            r"|(?:\b(?:app|this)\.set_(?:[A-Za-z_]\w*_modal|remote_browse)\s*\()"
        ),
        globs=("src/ui/**/*.rs",),
        excludes=(
            "src/ui/operations/modal_state.rs",
            "src/ui/operations/modal_state/",
        ),
        message=(
            "raw active_modal mutation or async receiver modal replacement bypasses slot "
            "arbitration — add a typed transition in operations/modal_state*"
        ),
        samples=(
            "app.active_modal = Some(modal);",
            "self.active_modal.replace(modal);",
            "match &mut self.active_modal { _ => {} }",
            (
                "cx.spawn(async move |this, cx| {\n"
                "    this.update(cx, |app, _| app.set_push_modal(modal));\n"
                "})"
            ),
        ),
        samples_ok=(
            "if app.active_modal.is_none() { offer_plan(); }",
            "match &app.active_modal { _ => {} }",
            "self.set_push_modal(modal);",
        ),
        path_samples=(
            ("src/ui/operations/modal_state.rs", True),
            ("src/ui/operations/modal_state/window.rs", True),
            ("src/ui/operations/pull_push.rs", False),
        ),
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
        name="hover-tooltip",
        summary="Kagi explanatory tooltips use the live-trigger boundary (#1591)",
        pattern=r"\.\s*tooltip(?:_with_action|_builder)?\s*\(",
        globs=("src/**/*.rs", "crates/kagi-ui-*/src/**/*.rs"),
        excludes=("crates/kagi-ui-core/src/tooltip.rs",),
        message=(
            "use HoverTooltip::hover_tooltip / hover_tooltip_builder after configuring the control"
        ),
        samples=(
            'button.tooltip("Help")',
            "button.tooltip_builder(build)",
            "button.tooltip_with_action(build, action)",
            "button.\n    tooltip_builder\n    (build)",
        ),
        samples_ok=(
            "button.hover_tooltip(id, text)",
            "button.hover_tooltip_builder(id, build)",
            "button.hoverable_tooltip(build)",
        ),
        path_samples=(
            ("crates/kagi-ui-core/src/tooltip.rs", True),
            ("src/ui/render_header.rs", False),
            ("crates/kagi-ui-editor/src/tabs.rs", False),
        ),
    ),
    Rule(
        name="static-spinner",
        summary="no rotating-arrow glyph standing in for a spinner (user report)",
        # U+27F3 is a *character*: it cannot turn, so an in-flight indicator
        # drawn with it reads as an operation that hung. Every spinner goes
        # through `render_overlay::sync_spinner`, which animates the SVG (and
        # deliberately stands still under reduce-motion).
        pattern=r'"[^"\n]*(?:\\u\{27f3\}|\u27f3)',
        globs=RUST_SOURCES,
        excludes=("ci/",),
        message=(
            "a rotating-arrow glyph cannot rotate - use "
            "render_overlay::sync_spinner for an in-flight indicator"
        ),
        samples=(
            'SharedString::from(format!("\\u{27f3} {}", msg))',
            'ToastKind::Info => (theme().color_branch, "\u27f3"),',
        ),
        samples_ok=('sync_spinner(10., footer_color, "footer-busy")',),
    ),
    Rule(
        name="string-continuation",
        summary="a `\\` line continuation never glues two words in a string (#843)",
        # Rust's `\`+newline also strips the next line's leading blanks, so a
        # word before the backslash and a word starting the next line become
        # one token: GraphQL's `mergeStateStatus\` + `   reviewThreads` reached
        # GitHub as `mergeStateStatusreviewThreads` and every merge-status read
        # failed (#843; #837 found the same in the thread query). Only a word
        # character right before the backslash counts, and not one that is
        # itself an escape (`\n\`, `\t\`): those end the line on purpose.
        pattern=r"(?<!\\)[A-Za-z0-9_]\\\n[ \t]*[A-Za-z0-9_]",
        globs=("crates/**/*.rs", "src/**/*.rs"),
        message=(
            "a `\\` continuation drops the next line's indentation and glues the "
            "two words - end the line with whitespace or punctuation, or use a "
            "plain newline in the string"
        ),
        samples=(
            '"\\\n   id mergeStateStatus\\\n   reviewThreads(first:100){nodes{isResolved}}\\\n"',
            '"  mergeQueueEntry{position estimatedTimeToMerge state\\\n'
            '    mergeQueue{nextEntryEstimatedTimeToMerge}}}}}";',
        ),
        samples_ok=(
            '"first line\\n\\\n   second line"',
            '"text\\\n\n   after a blank line"',
            '"query{\\\n   field}"',
        ),
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
        name="ui-settings-host-boundary",
        summary="shared Settings renders host snapshots without native effects (#1816)",
        # Scan every cfg branch: native feature unification must not expose a
        # back door. Match forbidden imported symbols too, before aliasing can
        # hide a call; reject core glob imports at this narrow boundary.
        pattern=(
            r"\b(?:std|tokio|async_std)\s*::\s*(?:fs|io|process|net|env|os)\b"
            r"|\b(?:fs|process|env|git2|kagi_git|theme_custom|system_accent|store)\s*::"
            r"|\b(?:theme_custom|system_accent|pr_viewed)\b"
            r"|\.\s*(?:exists|try_exists|is_file|is_dir|read_dir|read_link"
            r"|canonicalize|metadata|symlink_metadata)\s*\("
            r"|\b(?:File|OpenOptions|Command|Repository)\s*::"
            r"|\buse\b[^;]*\b(?:fs|io|process|net|env|os)\b"
            r"|\buse\b[^;]*\b(?:kagi_ui_core|theme|i18n|settings)\b[^;]*\*"
            r"|\b(?:extern\s+crate\s+(?:git2|kagi_git)|Settings\s*::\s*load)\b"
            r"|\bSettings\s+as\b"
            r"|\bsettings\s*::\s*(?:read|write|flush|store)\b"
            r"|\buse\b[^;]*\bsettings\b[^;]*\b(?:read|write|flush|store)\b"
            r"|\b(?:read_setting|write_setting|settings_path|analyze_ignore_path"
            r"|read_analyze_ignore_text|write_analyze_ignore|migrate_ui_scale_base"
            r"|main_window_frame|write_main_window_frame"
            r"|settings_window_frame|write_settings_window_frame"
            r"|read_custom_themes|themes_dir|read_col_width"
            r"|reload_custom_themes|install_custom_themes|take_theme_load_errors"
            r"|init_active|init_loaded_active|init_loaded_zoom|init_loaded_accent"
            r"|set_zoom|set_active|set_lang|set_lang_no_persist|resolve_lang"
            r"|set_compact_threshold_rem|set_col_width|set_compact_graph"
            r"|set_diff_split|set_graph_lane_compact|set_auto_fetch|set_reduce_motion"
            r"|set_toolbar_display|set_terminal_auto_lock|set_follows_system_accent"
            r"|refresh_system_accent|detected_system_accent"
            r"|init_zoom|init_lang|init_compact_threshold_rem|init_compact_graph"
            r"|init_diff_split|init_graph_lane_compact|init_auto_fetch"
            r"|init_reduce_motion|init_toolbar_display|init_terminal_auto_lock)\b"
        ),
        globs=("crates/kagi-ui-settings/**/*.rs",),
        message=(
            "Shared Settings may only read presentation state and emit typed intents; "
            "storage, discovery and application/persistence belong to the host (ADR-0121)."
        ),
        select_source=lambda text: _rust_literal_spans(text)[0],
        samples=(
            'std::fs::read("settings.json");',
            "use std::{fs as disk};",
            "use std::process::Command as Child;",
            "use kagi_ui_core::theme_custom as custom; custom::load_dir(path);",
            "use kagi_ui_core::pr_viewed as viewed; viewed::load(repo, number);",
            "path.exists();",
            "use kagi_ui_core::settings::{flush as save};",
            "Command::new(program).spawn();",
            "git2::Repository::open(path);",
            "kagi_git::Backend::new();",
            "theme::set_zoom(1.0);",
            "theme::set_active(slug);",
            "i18n::set_lang(Lang::En);",
            "use kagi_ui_core::theme::{set_zoom as apply};",
            "use kagi_ui_core::{theme as palette}; palette::set_active(slug);",
            "use kagi_ui_core::theme::*;",
            "settings::read(key);",
            "settings::write(key, value);",
            "settings::read_setting(key);",
            "settings::write_setting(key, value);",
            "settings::flush();",
            "Settings::load();",
            "use kagi_ui_core::settings::Settings as Preferences;",
            "theme::read_custom_themes();",
            "theme::themes_dir();",
            "theme::refresh_system_accent();",
            "theme_custom::load_dir(path);",
            "theme::reload_custom_themes();",
            "theme::install_custom_themes(themes);",
            "theme::init_loaded_active(slug);",
            "theme::init_loaded_zoom(1000);",
            "theme::init_loaded_accent(true);",
            "i18n::set_lang_no_persist(Lang::En);",
            '#[cfg(not(target_family = "wasm"))]\nfn native() { theme::init_zoom(); }',
        ),
        samples_ok=(
            "use gpui::prelude::*; use gpui_component::*;",
            "use kagi_ui_core::{i18n, theme, keyboard_nav};",
            "use kagi_ui_core::theme::{theme, scaled_px};",
            "theme(); scaled_px(12.0); i18n::lang();",
            "theme::theme(); theme::scaled_px(12.0);",
            "use kagi_ui_core::settings::{ToolbarDisplay, MarkdownDefaultView};",
            "let mode = ToolbarDisplay::IconOnly;",
            '// Host calls theme::set_zoom; shared renderer emits an intent.\n',
            'let help = "Settings::load() belongs to the native host";',
        ),
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
        name="recovery-safe-advice",
        summary="recovery guidance never recommends a hard reset (#456)",
        # Plan-recovery strings are intentionally scanned, not every occurrence of
        # the words: safety documentation may correctly say that hard reset is
        # forbidden. The recovery verbs identify user-facing instructions. Bound
        # the search to one recovery block, including Rust's escaped `\\n` lines,
        # so the rule catches multi-line literals without treating comments as UI.
        pattern=(
            r"(?i)(?:"
            r"(?:to (?:restore|undo)|recoverable)(?:.|\n){0,400}?"
            r"(?:(?:\\n|[\r\n]+)\s*git\s+reset\s+--hard\b|/\s*reset\s+--hard\b)"
            r"|(?:元に戻すには|復元するには|取り消すには)(?:.|\n){0,400}?"
            r"(?:\\n|[\r\n]+)\s*git\s+reset\s+--hard\b"
            r"|[\"']git\s+reset\s+--hard\b"
            r")"
        ),
        globs=(
            "crates/kagi-domain/src/plan_note/**/*.rs",
            "crates/kagi-git/src/**/*.rs",
            "crates/kagi-ui-core/src/i18n/plan/**/*.rs",
            "src/ui/modal_renderers_destructive.rs",
            "src/ui/operations/history.rs",
        ),
        message=(
            "Recovery guidance must not recommend `git reset --hard`; use a safe ref move, "
            "a recovery branch, or revert instead (#456)."
        ),
        samples=(
            "To restore the original commit:\n  git reset --hard deadbeef",
            "To restore the original commit:\\n  git reset --hard deadbeef",
            "実行後に merge commit を取り消すには:\n  git reset --hard HEAD~1",
            "The old commit is recoverable via git reflog / reset --hard <old>.",
            'commands: vec![format!("git reset --hard {}", old_short)]',
            'commands: vec!["git reset --hard HEAD~1".to_string()]',
        ),
        samples_ok=(
            "To restore without changing the tree:\\n  git reset --soft deadbeef",
            "This is a safe ref move (no reset --hard, ever).",
            'commands: vec!["git revert -m 1 HEAD".to_string()]',
        ),
        flags=re.MULTILINE,
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
        # #1381 / ADR-0105: this shared helper intentionally shows 25 rows
        # plus a visible overflow line. Other modal lists still render every row.
        excludes=("src/ui/modal_preview_files.rs",),
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
        name="e2e-window-helper",
        summary="GUI E2E windows all go through the budgeted open_offscreen helper",
        # gpui's `open_offscreen_window` hardcodes `show: true` and counts
        # nothing, so a nested loop can open thousands of real NSWindows — a
        # full run once opened 1,408 and the WindowServer watchdog killed the
        # user's session (#549). `macos::open_offscreen` sets `show: false`
        # unless KAGI_GUI_E2E_VISIBLE=1 and panics past MAX_LIVE_WINDOWS.
        #
        # Matched with the leading dot, i.e. the *call*; prose naming the gpui
        # method as `VisualTestAppContext::open_offscreen_window` is fine.
        pattern=r"\.\s*open_offscreen_window\s*\(",
        globs=("tests/**/*.rs",),
        message=(
            "tests must open GUI E2E windows through `macos::open_offscreen` "
            "(hidden by default + live-window budget), never gpui's raw "
            "`.open_offscreen_window(` — see #549."
        ),
        samples=(
            "let win = cx.open_offscreen_window(size(px(640.), px(480.)), f).unwrap();",
            # The rustfmt-wrapped form, which a line-based gate would miss.
            "let win = cx\n"
            "    .open_offscreen_window(size(px(w), px(h)), f)\n"
            '    .expect("open row matrix window");',
        ),
        samples_ok=(
            "let win = crate::macos::open_offscreen(cx, size(px(w), px(h)), f);",
            "/// gpui's own `VisualTestAppContext::open_offscreen_window` hardcodes `show: true`.",
        ),
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


def workspace_dep_aliases(root: Path = ROOT) -> dict[str, str]:
    """`[workspace.dependencies]` alias -> effective crate name, from the root manifest.

    Round-2 review: a `{ workspace = true }` dependency carries no `package`
    of its own, so a rename declared once in the root
    (`backend = { package = "kagi-git" }`) made every inheriting crate look
    like it depended on `backend` — a clean bypass of both layering gates.
    """
    root_manifest = root / "Cargo.toml"
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
    # The root `[workspace.dependencies]` table the *samples* resolve against.
    # A `{ workspace = true }` dependency carries no `package` of its own, so
    # the inherited rename only exists in that table: without a map here, no
    # sample can state the inherited case at all. Samples are judged against
    # this map and never against the live root manifest, so a sample means the
    # same thing in every checkout — `hits()` still reads the real table.
    sample_aliases: tuple[tuple[str, str], ...] = ()
    # A closed dependency surface is stronger than an ever-growing denylist.
    allowed: tuple[str, ...] | None = None

    def dependency_hits(
        self, text: str, aliases: dict[str, str] | None = None
    ) -> list[tuple[str, str]]:
        if self.allowed is None:
            return manifest_dep_hits(text, self.banned, aliases)
        allowed = {crate_name(name) for name in self.allowed}
        return [
            (key, name)
            for key, name in manifest_dep_names(text, aliases)
            if crate_name(name) not in allowed
        ]

    def fires_on(self, text: str) -> bool:
        return bool(self.dependency_hits(text, dict(self.sample_aliases)))

    def hits(self) -> list[tuple[Path, str, str]]:
        """(file, declared key, banned crate name) for every offending dependency."""
        out: list[tuple[Path, str, str]] = []
        for rel in iter_files(list(self.globs), self.excludes):
            for key, name in self.dependency_hits(read_text(rel)):
                out.append((rel, key, name))
        return out


MANIFEST_RULES: tuple[ManifestRule, ...] = (
    ManifestRule(
        name="ui-settings-host-manifest",
        summary="shared Settings has only its four presentation dependencies (#1816)",
        globs=("crates/kagi-ui-settings/**/Cargo.toml",),
        banned=(),
        allowed=("kagi-domain", "kagi-ui-core", "gpui", "gpui-component"),
        message="kagi-ui-settings may depend only on domain, UI core, GPUI and gpui-component.",
        samples=(
            '[dependencies]\nkagi-git = "0.1"\n',
            '[dependencies]\ndisk = { package = "tokio", version = "1" }\n',
            '[target.\'cfg(unix)\'.dependencies]\nlibc = "0.2"\n',
            '[build-dependencies]\ncc = "1"\n',
            '[dev-dependencies]\ntempfile = "3"\n',
            '[dependencies]\nbackend.workspace = true\n',
            '[dependencies.disk]\npackage = "std-fs"\nversion = "1"\n',
        ),
        samples_ok=(
            '[dependencies]\nkagi-domain = { path = "../kagi-domain" }\n'
            'kagi-ui-core = { path = "../kagi-ui-core" }\n'
            'gpui = "0.2"\ngpui-component = "0.5"\n',
            '[dependencies]\nui.workspace = true\n',
        ),
        sample_aliases=(("backend", "kagi-git"), ("ui", "gpui")),
    ),
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
            # The rename lives in the root table, so this manifest names only
            # `backend` — the bypass `workspace_dep_aliases` exists to close.
            "[dependencies]\nbackend = { workspace = true }\n",
        ),
        samples_ok=(
            '[dependencies]\nkagi-ui-core = { path = "../kagi-ui-core" }\n',
            # Inherited too, and resolved through the same table: the map is
            # consulted and yields an allowed crate, so `{ workspace = true }`
            # is not banned wholesale.
            "[dependencies]\nui = { workspace = true }\n",
        ),
        sample_aliases=(("backend", "kagi-git"), ("ui", "kagi-ui-core")),
    ),
    ManifestRule(
        name="mcp-gpui-manifest",
        summary="crates/kagi-mcp manifests never depend on gpui (ADR-0163 / #331)",
        globs=("crates/kagi-mcp/**/Cargo.toml",),
        banned=("gpui",),
        message="crates/kagi-mcp must not depend on gpui (ADR-0163 / #331).",
        samples=(
            '[dependencies]\nui = { package = "gpui", git = "https://github.com/zed" }\n',
            "[dependencies]\nui = { workspace = true }\n",
        ),
        samples_ok=(
            '[dependencies]\ngpui-component = { git = "https://example" }\n',
            "[dependencies]\ncomponent = { workspace = true }\n",
        ),
        sample_aliases=(("ui", "gpui"), ("component", "gpui-component")),
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


# Files the LOC gate looks at. There is no baseline file any more (2026-10-01,
# docs/decisions.md): a committed per-file ceiling conflicted on every merge
# and was accepted on every bump, so it recorded growth instead of stopping
# it. The gate now fails only for a file that is **new in the branch** and over
# the ceiling; existing files are reported as notices and split by issue.
LOC_GLOBS = ("src/**/*.rs", "crates/*/src/**/*.rs")

LOC_SAMPLES: tuple[tuple[str, int], ...] = (
    ("fn f() {}\n" * (LOC_CEILING + 1), LOC_CEILING + 1),
    ("fn f() {}\n" * LOC_CEILING, 0),
)


def loc_selftest() -> list[str]:
    """The counter must still count: an unguarded one zeroes every file."""
    issues: list[str] = []
    for sample, expected in LOC_SAMPLES:
        found = loc_count(sample)
        if found != expected:
            issues.append(
                f"loc_count returned {found} for a {len(sample.splitlines())}-line "
                f"sample, expected {expected}"
            )
    return issues


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
)


# ── Custom checks: rules a single regex cannot express ──────────────────────


def ui_lateral_crates(root: Path = ROOT) -> list[Path]:
    return sorted(d for d in (root / "crates").glob("kagi-ui-*") if d.is_dir())


def ui_lateral_hits(root: Path = ROOT) -> list[tuple[Path, int, str, str]]:
    """Feature crates may import kagi-ui-core, never a sibling kagi-ui-* crate.

    The allowed set depends on which crate is being scanned, which is why this
    is a function and not a `Rule`. `kagi-ui-core` is the shared base, and a
    crate naming itself is not an import (ADR-0121).

    `root` is the workspace to scan; `ui_lateral_selftest` points it at a
    fixture so the pattern, the allowed set and the normalisation are proven
    against known hits instead of the repository's happy zero (#470).
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
    for crate in ui_lateral_crates(root):
        hyphen = crate.name
        allowed = {"kagi-ui-core", hyphen}
        for rel in iter_files([f"crates/{hyphen}/**/*.rs"], root=root):
            text = read_text(rel, root)
            lines = text.splitlines()
            for match in rust.finditer(text):
                name = next(g for g in match.groups() if g)
                if crate_name(name) in allowed:
                    continue
                line_no = text.count("\n", 0, match.start()) + 1
                out.append((rel, line_no, lines[line_no - 1].strip(), hyphen))
    return out


def ui_lateral_manifest_hits(root: Path = ROOT) -> list[tuple[Path, str, str, str]]:
    """(manifest, declared key, sibling crate, scanned crate) for lateral deps.

    Parsed, not matched, for the reason `manifest_dep_names` exists: a rename
    (`sib = { package = "kagi-ui-editor" }`) is a lateral dependency that no
    dependency-line pattern can see. The scanned workspace's own
    `[workspace.dependencies]` table is resolved once and handed down, so an
    inherited rename is read from the root being scanned rather than whichever
    manifest happens to sit at `ROOT`.
    """
    aliases = workspace_dep_aliases(root)
    out: list[tuple[Path, str, str, str]] = []
    for crate in ui_lateral_crates(root):
        hyphen = crate.name
        allowed = {"kagi-ui-core", hyphen}
        for rel in iter_files([f"crates/{hyphen}/**/Cargo.toml"], root=root):
            for key, name in manifest_dep_names(read_text(rel, root), aliases):
                norm = crate_name(name)
                if not norm.startswith("kagi-ui-") or norm in allowed:
                    continue
                out.append((rel, key, norm, hyphen))
    return out


def ui_lateral_crate_count(root: Path = ROOT) -> int:
    return len(ui_lateral_crates(root))


def ui_lateral_selftest() -> list[str]:
    """Drive the real lateral walkers over a fixture workspace.

    The gate's only guard was "did it find any kagi-ui-* crates", so a
    regression in the Rust pattern, the manifest traversal, the hyphen /
    underscore normalisation or the allowed set returned no hits while the
    crates still existed — and printed OK (#470). The fixture holds one
    lateral import, one inherited rename and one direct rename, with every
    allowed shape beside them; the violations are then removed and the same
    walkers must go quiet with the crates still in place.
    """
    lateral_use = "use kagi_ui_editor::Buffer;\n"
    inherited_dep = "sib = { workspace = true }\n"
    renamed_dep = 'editor = { package = "kagi-ui-editor", path = "../kagi-ui-editor" }\n'
    history_lib = (
        "//! moved here from `kagi-ui-editor` (once `kagi_ui_editor`): prose naming a\n"
        "//! sibling is not an import — the #443 false-positive class.\n"
        "use kagi_ui_core::Theme;\n"
        f"{lateral_use}"
        "\n"
        "pub fn rows(theme: &Theme) -> Buffer {\n"
        "    kagi_ui_file_history::build(theme)\n"
        "}\n"
    )
    # `kagi_ui_editor` under [dev-dependencies] is the crate's own name in the
    # underscore spelling: allowed, and only the normalisation makes it so.
    editor_manifest = (
        '[package]\nname = "kagi-ui-editor"\n\n'
        "[dependencies]\n"
        'kagi-ui-core = { path = "../kagi-ui-core" }\n'
        f"{inherited_dep}"
        "\n[dev-dependencies]\n"
        'kagi_ui_editor = { path = "." }\n'
    )
    history_manifest = (
        '[package]\nname = "kagi-ui-file-history"\n\n'
        "[dependencies]\n"
        'kagi_ui_core = { path = "../kagi-ui-core" }\n'
        f"{renamed_dep}"
    )
    with TemporaryDirectory() as directory:
        root = Path(directory)

        def write(rel: str, text: str) -> None:
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")

        write(
            "Cargo.toml",
            "[workspace]\n"
            'members = ["crates/kagi-ui-core", "crates/kagi-ui-editor", '
            '"crates/kagi-ui-file-history"]\n\n'
            "[workspace.dependencies]\n"
            'sib = { package = "kagi-ui-file-history", path = "crates/kagi-ui-file-history" }\n',
        )
        write("crates/kagi-ui-core/Cargo.toml", '[package]\nname = "kagi-ui-core"\n')
        write("crates/kagi-ui-core/src/lib.rs", "pub struct Theme;\n")
        write("crates/kagi-ui-editor/Cargo.toml", editor_manifest)
        write(
            "crates/kagi-ui-editor/src/lib.rs",
            "use kagi_ui_core::Theme;\n"
            "\n"
            "pub fn open(theme: &Theme) {\n"
            "    kagi_ui_editor::detail::apply(theme);\n"
            "}\n",
        )
        write("crates/kagi-ui-file-history/Cargo.toml", history_manifest)
        write("crates/kagi-ui-file-history/src/lib.rs", history_lib)

        expected_crates = ["kagi-ui-core", "kagi-ui-editor", "kagi-ui-file-history"]
        crates = [crate.name for crate in ui_lateral_crates(root)]
        if crates != expected_crates:
            # Every assertion below is vacuous without the crates, so stop here.
            return [f"fixture root yielded {crates}, expected {expected_crates}"]

        issues: list[str] = []
        history_source = Path("crates/kagi-ui-file-history/src/lib.rs")
        expected_hits = [
            (
                history_source,
                history_lib.splitlines().index(lateral_use.strip()) + 1,
                lateral_use.strip(),
                "kagi-ui-file-history",
            )
        ]
        hits = ui_lateral_hits(root)
        if hits != expected_hits:
            issues.append(f"source walker returned {hits}, expected {expected_hits}")
        expected_manifest_hits = [
            (
                Path("crates/kagi-ui-editor/Cargo.toml"),
                "sib",
                "kagi-ui-file-history",
                "kagi-ui-editor",
            ),
            (
                Path("crates/kagi-ui-file-history/Cargo.toml"),
                "editor",
                "kagi-ui-editor",
                "kagi-ui-file-history",
            ),
        ]
        manifest_hits = ui_lateral_manifest_hits(root)
        if manifest_hits != expected_manifest_hits:
            issues.append(
                f"manifest walker returned {manifest_hits}, expected {expected_manifest_hits}"
            )

        # The same tree minus exactly its three violations. Every allowed shape
        # stays — the crate's own name in source and in a manifest key,
        # kagi-ui-core in both spellings, and the prose mention — so anything
        # still reported here is the allowed-set logic failing open the other way.
        write("crates/kagi-ui-file-history/src/lib.rs", history_lib.replace(lateral_use, ""))
        write("crates/kagi-ui-editor/Cargo.toml", editor_manifest.replace(inherited_dep, ""))
        write("crates/kagi-ui-file-history/Cargo.toml", history_manifest.replace(renamed_dep, ""))
        if ui_lateral_crate_count(root) != len(expected_crates):
            issues.append("the cleaned fixture lost its kagi-ui-* crates — nothing was scanned")
        clean_hits = ui_lateral_hits(root)
        if clean_hits:
            issues.append(f"allowed imports reported as lateral: {clean_hits}")
        clean_manifest_hits = ui_lateral_manifest_hits(root)
        if clean_manifest_hits:
            issues.append(f"allowed dependencies reported as lateral: {clean_manifest_hits}")
    return issues


# ── ADR numbering: one 4-digit number, one ADR ──────────────────────────────

ADR_DIR = "docs/adr"

# The canonical filename shape: `docs/adr/NNNN-slug.md`. Scope is deliberately
# the 4-digit prefix, because that prefix *is* the numbering convention — a
# gate on it is a gate on the convention. `ADR-0097-web-e2e-harness.md` uses an
# older spelling and is outside this gate; if a second ADR-prefixed file ever
# appears, rename it to the canonical form rather than widening the pattern
# (#620).
ADR_NAME = re.compile(r"^(\d{4})-.*\.md$")

# The duplicates that already existed when this gate landed (#620), pinned to
# the exact pair of filenames approved at each number.
# 以前からの重複。番号を動かすと参照が広範に動くため据え置き。
# (Renumbering any of these would move references across `.rs`, `.md`,
# `AGENTS.md` and `ci/` for no behavioural gain. The two pairs #620 *did*
# renumber were days old, so their references were still cheap to move.)
#
# Filenames, not bare numbers: a number-only exemption would also license a
# *third* ADR at that number, which is the very collision this gate exists to
# stop. Adding or renaming a file at a grandfathered number therefore fails
# until the pair below is updated on purpose.
ADR_GRANDFATHERED: dict[str, frozenset[str]] = {
    "0073": frozenset(
        {"0073-git-backend-trait-operation-pipeline.md", "0073-repo-worker-thread.md"}
    ),
    "0089": frozenset({"0089-file-history.md", "0089-remote-ssh-git-backend.md"}),
    "0104": frozenset({"0104-enforced-operation-pipeline.md", "0104-swimlane-compaction.md"}),
    "0129": frozenset({"0129-appendix-templates.md", "0129-plan-note-i18n.md"}),
    "0130": frozenset({"0130-bundled-japanese-font-fallback.md", "0130-force-with-lease-push.md"}),
    "0175": frozenset({"0175-app-remove-boundary.md", "0175-flower-road-lane-palettes.md"}),
}


def adr_groups(names: Iterable[str]) -> dict[str, list[str]]:
    """4-digit ADR number -> the canonical filenames carrying it."""
    groups: dict[str, list[str]] = {}
    for name in names:
        match = ADR_NAME.match(name)
        if match:
            groups.setdefault(match.group(1), []).append(name)
    return {number: sorted(files) for number, files in groups.items()}


def adr_file_names() -> list[str]:
    """Every Markdown file in `docs/adr`, by name (numbering is in the name)."""
    directory = ROOT / ADR_DIR
    if not directory.is_dir():
        return []
    return sorted(path.name for path in directory.glob("*.md"))


def adr_duplicate_hits(
    names: Iterable[str],
    allowlist: dict[str, frozenset[str]] | None = None,
) -> list[tuple[str, list[str]]]:
    """(number, files) for each number shared by ADRs the allowlist does not approve.

    A grandfathered number is exempt only while its files are *exactly* the
    approved pair — a third ADR, or a renamed one, is a fresh collision.
    """
    approved = ADR_GRANDFATHERED if allowlist is None else allowlist
    return [
        (number, files)
        for number, files in sorted(adr_groups(names).items())
        if len(files) > 1 and frozenset(files) != approved.get(number)
    ]


def adr_stale_allowlist(
    names: Iterable[str],
    allowlist: dict[str, frozenset[str]] | None = None,
) -> list[str]:
    """Allowlisted numbers that no longer hold a duplicate at all.

    Without this the allowlist rots in the dangerous direction: a duplicate
    resolved later leaves its number permanently exempt, silently licensing the
    *next* collision there.
    """
    approved = ADR_GRANDFATHERED if allowlist is None else allowlist
    groups = adr_groups(names)
    return sorted(number for number in approved if len(groups.get(number, ())) < 2)


def adr_selftest() -> list[str]:
    """Prove the gate flags a duplicate, clears a renumbered set, and honours the allowlist.

    Fixtures use a synthetic `9000`/`9001` pair, never a real ADR filename: a
    fixture spelling a moved ADR's old name would keep that name alive in the
    repository, which is exactly what a renumber has to remove (#620).
    """
    issues: list[str] = []
    none: dict[str, frozenset[str]] = {}
    # One number held twice, and the same set after the newer ADR moves off it.
    duplicate = ["9000-first-decision.md", "9000-second-decision.md"]
    renumbered = ["9000-first-decision.md", "9001-second-decision.md"]
    pinned = {"9000": frozenset(duplicate)}
    if not adr_duplicate_hits(duplicate, none):
        issues.append(f"no longer flags a duplicate number: {duplicate}")
    if adr_duplicate_hits(renumbered, none):
        issues.append(f"flags unique numbers as duplicates (false positive): {renumbered}")
    if adr_duplicate_hits(duplicate, pinned):
        issues.append("the approved grandfathered pair is reported as a duplicate")
    # The reason the allowlist pins filenames: a third ADR at a grandfathered
    # number is a new collision, not part of the exemption.
    if not adr_duplicate_hits([*duplicate, "9000-third-decision.md"], pinned):
        issues.append("a third ADR at a grandfathered number is not flagged")
    # …and so is a rename that leaves the pair no longer the approved one.
    if not adr_duplicate_hits(["9000-first-decision.md", "9000-renamed.md"], pinned):
        issues.append("a renamed file at a grandfathered number is not flagged")
    if adr_stale_allowlist(duplicate, pinned):
        issues.append("a live duplicate is reported as a stale allowlist entry")
    if adr_stale_allowlist(renumbered, pinned) != ["9000"]:
        issues.append("a resolved duplicate is no longer reported as a stale allowlist entry")
    # And that the real directory is still visible to the gate.
    if not adr_groups(adr_file_names()):
        issues.append(f"{ADR_DIR} yielded no numbered ADRs — the gate would scan nothing")
    return issues
