# ADR-0172 — Local apply of a GitHub PR review "suggested change"

Status: Accepted
Issue: #351 (parent #359)
Date: 2026-09-04

## Context

GitHub PR reviews carry ```suggestion blocks — concrete, applyable code
proposals (the Copilot / Codex surface). GitHub's own Web UI can only *commit*
a suggestion directly. Kagi's edge (#351 §2) is to apply it **locally** into the
working tree, under oplog / ODB backup, so the user then selects with the
existing hunk-staging UI before committing.

This ADR covers only the **backend slice**: parse a suggestion and apply it to
the working-tree file, safely, through the write pipeline. The diff-overlay
rendering, multi-suggestion batch UI, and per-file viewed-state tracking are
deferred to follow-up GUI slices.

## Decision

### 1. Pure parser (`kagi-domain`)

`crates/kagi-domain/src/github.rs` gains a pure `Suggestion { path, start_line,
end_line, replacement }` and:

- `parse_suggestion(body, path, start_line, line) -> Option<Suggestion>` — pulls
  the ```suggestion fence out of a review-comment body. The anchor
  (`path` / `start_line` / `line`) is passed in by the caller from the `gh`
  review data, so the parser is pure over its inputs and testable without `gh`.
  An unterminated fence → `None`; an empty fence → `Some` with empty
  `replacement` (a deletion). Fence indentation (Markdown list items) is
  stripped.
- `Suggestion::apply_to(original) -> Option<String>` — splices the replacement
  into the 1-based inclusive `[start_line, end_line]` range; preserves a
  trailing newline; `None` on an out-of-bounds / inverted range.
- `line_range(content, start, end) -> Option<Vec<String>>` — the anchored lines,
  used both to capture the baseline at plan time and to re-read it at execute
  time.

`ReviewComment` gains `start_line: Option<u32>` (parsed from the API's
`start_line` / `original_start_line`) and a `suggestion()` convenience.

### 2. Op triple (`crates/kagi-git/src/ops/suggestion.rs`)

`plan_apply_suggestion` / `preflight_apply_suggestion` /
`execute_apply_suggestion`, plus `capture_suggestion_context` (reads the
anchored lines at plan time). Wired through `Backend::run` via a new
`Operation::ApplySuggestion { suggestion, expected_original }` and
`OperationOutcome::Suggestion(SuggestionOutcome)`, so it inherits the trust gate,
HEAD preflight, and single-writer oplog recording (`op="apply-suggestion"`).

The apply writes **only the working tree** — nothing is staged or committed
(`destructive: false`; selection happens later via hunk staging). The pre-apply
file content is backed up to the ODB (`repo.blob`) first; the blob SHA is the
recovery handle carried in `SuggestionOutcome` and the oplog.

### 3. Stale-line safety (TOCTOU, the critical guard)

`expected_original` — the anchored range's content at plan time — is captured via
`capture_suggestion_context` and threaded into the `Operation`. Both `plan`
(blocker `SuggestionStale`) and, authoritatively, `execute` re-read the
working-tree range and compare it to `expected_original`. If it no longer
matches, execute **refuses** with an error naming the "stale range" and writes
nothing — a suggestion is never spliced onto the wrong lines (same class as
#393 / #405). This is asserted in `tests/suggestion_apply_test.rs`
(`stale_range_after_plan_makes_execute_refuse`), where the file is edited *after*
the plan is built.

## Consequences

- The invariants hold: git2 stays out of `src/ui/` and `kagi-ui-*`; `kagi-domain`
  stays pure (parser + splice logic + goldens live there); no destructive
  commands; the write goes through `plan → confirm → preflight → execute →
  verify → oplog`. EN + JA strings added for the new `GithubNote` / `GithubTitle`
  / `GithubRecovery` variants.
- The `gh` fetch already exists (`github::pr_review_comments` /
  `parse_review_comments`); only `start_line` parsing was added.

## Deferred (GUI follow-ups, out of scope here)

- Rendering review threads / suggestions overlaid on the split diff at the
  anchored line (§4 "review thread の重畳").
- Applying multiple suggestions in a batch (GitHub's batch feature).
- Per-file viewed-state tracking with blob-SHA invalidation (§4 "viewed 管理") —
  needs a local store, not `settings.json`.
- The UI affordance (button + confirm modal) on a review comment that wires the
  `capture → plan → confirm → run` flow. Backend + tests are ready for it.
