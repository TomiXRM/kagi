# ADR-0106: Atomic conflict resolution save

- Status: Accepted
- Date: 2026-06-20
- Implements: `/docs/git-safety-checklist.md` §19
- Pattern source: `execute_conflict_continue`'s deferred-`index.write()`

## Context

`stage_conflict_resolution` (`src/git/conflicts.rs:931-984`) writes resolved
content per file in a loop:

```
for file in session.files:
    fs::write(file.path, file.resolved_text)?    // ① disk write
    index.add_path(file.path)                     // ② in-memory stage
index.write()?                                     // ③ single index flush at end
```

If `fs::write` for file 3 of 5 fails (read-only directory, disk full, race
with an external editor), files 1–2 are **already overwritten on disk** but
the index is never written. The working tree now has partial resolutions, the
index still shows conflicts, and the user's manual edits to files 1–2 are
replaced by the buffer's resolution. Conflict resolution is the exact place
"never lose user work" matters most.

(`/docs/codebase-review.md` §4 Finding 20.)

## Decision

Make the save atomic using temp-write-then-rename:

```
1. For each resolved file, write to `<file>.kagi-resolve-tmp-<n>` (a sibling temp).
   On any failure, delete all temps and return Err — no target touched.
2. Rename all temps to their targets atomically (rename is atomic on POSIX
   and Windows for same-filesystem moves).
3. index.add_path each target, then index.write() once (the existing pattern).
4. On any failure after step 1, roll back: re-extract original conflict
   markers from the index and rewrite the targets.
```

The temp files use a `.kagi-resolve-tmp-<n>` suffix on the same filesystem as
the target (rename across filesystems is not atomic). The index write stays
deferred (this part of the existing design is correct).

## Consequences

- A mid-loop disk failure leaves the working tree and index in their original
  conflict state — no partial resolution, no lost markers.
- Cost: one extra `rename` per file (negligible vs the `fs::write` already
  happening).
- The pattern matches `execute_conflict_continue`'s deferred-`index.write()`
  approach, extending it to the WT writes too.
- Temp file naming uses a `.kagi-resolve-tmp-` prefix so a crash leaves
  discoverable garbage that a future cleanup pass can sweep.

## Rollout

One commit in `src/git/conflicts.rs`. New test: inject a write failure on
file 3 of 5 (chmod the directory read-only mid-loop, or a mock) → no target
files modified, no index write, `Err` returned.
