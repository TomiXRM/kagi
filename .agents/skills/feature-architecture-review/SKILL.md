---
name: feature-architecture-review
description: >-
  Enforces an architecture-first workflow before ANY new feature is coded.
  Use whenever the user asks to add, implement, build, create, or support new
  functionality — even a single function. Also use when the user says "add X",
  "implement Y", "support Z", "create a new...", "we need a feature for...".
  Do NOT use for bug fixes, pure refactors, documentation, test-only changes,
  or dependency bumps. The skill outputs a 5-point architecture review that
  MUST be completed (and shown to the user) before writing any feature code.
  If the review finds the feature belongs in existing code, the agent extends
  that code rather than creating a parallel implementation.
---

# Feature Architecture Review

> Default workflow for every feature request. The goal is NOT to block
> features — it is to prevent the codebase from accumulating parallel
> implementations, duplicate state, and single-use abstractions. Most of the
ime the right answer is "extend the existing owner," not "create a new thing."

## When this skill applies

Applies to: any request that adds new user-facing or system behavior
("add hunk staging", "implement pull-strategy selector", "support LFS",
"add a notification system", "create a branch-protection check").

Does NOT apply to: bug fixes, refactors, doc changes, test additions,
dependency bumps, or configuration changes — those have no "new feature"
surface to gate.

## The mandatory pre-implementation review

Before writing any feature code, complete this review and show it to the
user. If any step reveals the feature belongs in existing code, stop and
extend that code instead.

### Step 1 — SEARCH: what already does this?

Grep the codebase for related behavior. Look for:
- Functions with similar names or responsibility (`rg "stage"`, `rg "discard"`).
- Existing abstractions the feature could ride on (a pipeline, a trait,
  a handler chain, a state machine).
- Duplicated logic solving the same problem under different names.

Record concrete `file:line` evidence. "Nothing found" must be justified
with the greps you ran, not assumed.

### Step 2 — IDENTIFY: who owns this responsibility?

Name the single module/struct/function that currently owns this concern.
If multiple owners exist, that itself is a finding — flag the duplication.

Examples for this codebase:
- Git mutation → `Backend::run` (ADR-0104), not a new manager.
- Plan preview → the existing `OperationPlan` + plan modal, not a new
  preview service.
- Working-tree read → `RepoSession::backend()` (ADR-0107), not a new
  status checker.
- Conflict resolution → the existing `ResolutionBuffer` + conflict FSM in
  `kagi-domain`, not a new resolution service.

### Step 3 — DECIDE: extend or create?

Answer one of:
- **EXTEND** — the feature belongs in the existing owner identified in
  Step 2. Add a method, a variant, a parameter. No new module.
- **CREATE** — no existing owner can reasonably absorb this. This is the
  only path that permits a new module/struct/service. It REQUIRES written
  justification citing the greps from Step 1 that prove no existing owner.

Default to EXTEND. Only CREATE when you can show, with evidence, that
every plausible existing owner is a bad fit.

### Step 4 — REFACTOR: consolidate first if needed

If Step 1 found duplication, consolidate it BEFORE adding the feature.
Adding new code on top of duplicated code multiplies the maintenance
surface. Examples:
- Two functions doing the same thing under different names → merge them,
  then extend the merged version.
- A UI state field duplicating domain state → delete the UI copy, source
  it from the domain.

### Step 5 — ADD: write the minimum new code

Only now, after steps 1–4, add the feature. The new code must be the
minimum required, living in the owner identified in Step 2 (or the new
module justified in Step 3).

## Anti-patterns to reject

When completing the review, actively check for and call out these
patterns. If you detect one, stop and propose the consolidation instead.

1. **"Manager" without a lifecycle owner** — a new struct that holds state
   but nothing constructs/destroys it at a clear boundary (tab open/close,
   app init, request scope). If you can't say "X creates it, Y destroys
   it," it's an orphan.

2. **UI state duplicating domain state** — a `bool`/`Vec` on the UI struct
   that mirrors a field already in the domain layer or the git backend.
   Source it from the owner, don't copy it.

3. **Git/I/O inside render or UI event handlers** — `Backend::open`,
   `Repository::open`, `std::fs::*`, network calls inside `render()`,
   `on_click`, or `on_drop`. These belong behind `Backend`/`RepoSession`,
   invoked via the operation pipeline or a background task.

4. **One-off helper for a single feature** — a `fn` or struct used by
   exactly one call site, where inlining or extending an existing helper
   would do. One-off helpers accumulate; they rarely earn their keep.

5. **Feature flag / conditional branch where a unified model fits** —
   adding `if new_feature { ... } else { ... }` when the new path could
   be a parameter or variant of the existing path. Unified models compose;
   flags fragment.

6. **Dead code preserved "just in case"** — keeping an unused function,
   field, or import because "we might need it." Delete it; git remembers.

7. **Same problem solved in multiple places under different names** —
   e.g., two diff-formatting paths, two status-refresh paths. This is the
   single most common drift source. Consolidate before extending.

## Output format

Show the user this completed checklist before writing feature code:

```
## Architecture Review: <feature name>

1. SEARCH: <what you grepped, what exists, file:line evidence>
2. OWNER: <the single module/struct that owns this, or "none found">
3. DECISION: EXTEND <owner> | CREATE <new module> (with justification)
4. REFACTOR: <duplication to consolidate first, or "none needed">
5. ANTI-PATTERNS CHECKED: <which of the 7 you checked, any found>
```

If the decision is CREATE, the justification must cite the greps that
prove no existing owner can absorb the feature. "It feels cleaner" is
not justification.

## Worked example (this codebase)

> Request: "Add per-hunk staging."

```
## Architecture Review: per-hunk staging

1. SEARCH:
   - rg "stage_file|stage_files|unstage" → src/git/staging.rs:74,127,756,798
     (file-level staging only; no hunk-level API exists)
   - rg "stage_hunk|hunk.*stage|apply.*cached" → 0 hits
   - diff hunks ARE parsed: src/git/staging.rs:690-735 (HunkRange extraction)
   - The plan modal preview uses FileStatus (file-level), not hunks
2. OWNER: src/git/staging.rs owns staging. The hunk data already lives there
   (parsed at :690) but has no `stage_hunks` entry point.
3. DECISION: EXTEND staging.rs — add `stage_hunks(path, &[HunkRange])` using
   `git apply --cached` on a generated patch. No new module.
4. REFACTOR: the hunk parsing at :690 is currently inline in the diff path;
   extract it into a reusable `fn hunks_for(repo, path) -> Vec<HunkRange>`
   so both the diff viewer and the new staging API share it.
5. ANTI-PATTERNS: checked #2 (no new UI state — hunk selection lives on the
   existing diff view), #4 (no one-off helper — extends staging.rs), #7
   (single staging owner, no parallel path).
```

This review takes 2 minutes and prevents the "parallel staging manager"
that a naive implementation would create.
