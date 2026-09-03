# ADR-0162 — Line-level blame + `.git-blame-ignore-revs`

Status: Accepted
Issue: #350 (親トラッキング #359)
Depends on / relates to: ADR-0089 (file history, commit-level), ADR-0119
(Analyze ownership), ADR-0132 / ADR-0137 (embedded editor), #354 (colour-blind
markers), #345 (absorb — shared line→commit foundation).

## Context

Kagi has an embedded editor but no way to see *who wrote each line, when, and
why*. `file_history.rs` (ADR-0089) is **commit-level** (the per-path log); this
ADR adds the **line-level** companion. A common pain point is that a bulk
reformat (introducing prettier/rustfmt) collapses every line's blame onto that
one commit — `.git-blame-ignore-revs` exists to fix this, but few clients honour
it.

## Decision

### Layering

- **`kagi-domain::blame`** (pure): the models — `BlameLine`, `BlameResult` — and
  the `.git-blame-ignore-revs` text parser `parse_blame_ignore_revs(&str) ->
  Vec<String>`. Parsing a text file is pure, so it lives in the pure crate with
  golden tests. No `git2`, no I/O.
- **`kagi-git::blame`** (git2): `blame_file(repo, path) -> BlameResult` via
  git2's in-process `Repository::blame_file`, plus `Backend::blame_file`. This
  is the **only** layer that opens `git2` and the only one that reads the
  ignore-revs file from disk.
- **UI**: a blame gutter column + inline (end-of-line) blame in the existing
  embedded editor pane — no new pane type (stays on the ADR-0120 framework).
  Consumes `BlameResult` and the i18n strings; contains no `git2`.

### git2 blame, not shell-out (PM-locked)

We use the **git2 `blame` API** (in-process), matching Kagi's single-git2-backend
design, rather than shelling out to `git blame --porcelain`. The cost is that
git2 has **no native ignore-revs support**, so we handle that ourselves (below).

### `.git-blame-ignore-revs` — v1 marks, does not re-attribute (PM-locked)

- **Auto-detect** a `.git-blame-ignore-revs` at the repository root; parse it
  ourselves.
- v1 **marks** every line whose attributed commit is in the ignore set
  (`BlameLine::ignored`) and reports how many **distinct** ignored commits
  actually took effect in the file (`BlameResult::ignored_revs`), surfaced as
  the "N revisions ignored" indicator (`i18n::blame_revisions_ignored`).
- **Full re-attribution** (walking past an ignored commit to the prior commit
  that last touched the line) is a **documented follow-up**, not v1. git2's
  blame cannot do this natively; doing it ourselves means re-blaming with
  `oldest_commit` bounds per ignored hunk — deferred.
- Ignore entries may be abbreviated; we prefix-match them against full commit
  ids (`is_ignored`). Ceiling noted in code (O(lines × entries)).

### Markers are symbols, not colour (PM-locked, aligns #354)

- Ignored lines: `*` (`IGNORED_MARK`, git's `blame.markIgnoredLines`).
- Unblamable lines: `?` (`UNBLAMABLE_MARK`, git's `blame.markUnblamableLines`),
  e.g. an uncommitted line (zero oid). Unblamable wins over ignored.
- The distinction is legible **without colour**, so it survives on a monochrome
  display and for colour-blind users.

### Performance (PM-locked)

- Inline blame is computed **async, visible-range only** — never blame 10k lines
  eagerly on file open. `BlameResult.lines` is line-ordered so the UI can slice
  the visible range directly. (Full-file `blame_file` is what the backend
  exposes; the UI throttles it to the viewport — a GUI concern verified by a
  human.)

### Diff algorithm

Local git is 2.50.1. `git blame --diff-algorithm=histogram` (git 2.53+) reduces
mis-attribution of moved code, but it is a CLI flag with **no git2 equivalent**,
so v1 uses git2's default. Adopting histogram is future work, shared with
ownership (ADR-0119).

## Consequences

- Blame is available to the editor without violating the git2-layering
  invariant.
- The reformat-blame problem is directly answered: those lines are flagged and
  counted, not silently trusted.
- ADR-0119 ownership is **not** refactored onto this foundation now; sharing the
  line→commit attribution with ownership / #345 absorb is noted as a follow-up.

## Follow-ups (explicitly not v1)

1. Full re-attribution past ignored commits.
2. `--diff-algorithm=histogram` equivalent.
3. Unify ADR-0119 ownership + #345 absorb onto this attribution foundation.
