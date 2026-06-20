# ADR-0108: Finish kagi-domain extraction; resolve filename collisions

- Status: Accepted
- Date: 2026-06-20
- Implements: `/docs/refactor-plan.md` Phase 2 Step 2.2
- Continues: ADR-0072 (workspace crate split)

## Context

The `kagi-domain` extraction (ADR-0072, migration steps S2a–S2d, S3a) moved
pure types and some pure logic to `crates/kagi-domain/`. The migration README
describes the `src/git/<x>.rs` files as "re-export shims."

The 2026-06-20 review (`/docs/codebase-review.md` §2 "Architecture Finding 8"
and §7 "Cleanup Finding 4") found these are **not pure shims** — they are
partly parallel implementations with real drift risk:

| File | domain LOC | src/git LOC | Issue |
|---|---|---|---|
| `history.rs` | 388 | 546 | **Filename collision.** Domain has `OperationKind`/`HistoryEntry`/`OperationHistory`; `src/git/history.rs` defines `FileHistoryRequest`/`FileHistory`/`FileHistoryEntry`/`CommitSummary` — two different concepts under one name. |
| `message_gen.rs` | 892 | 976 | Domain has rule logic; `src/git/` re-exports *and* adds 6 git2-backed fns. Boundary unclear. |
| `resolution.rs` | 905 | 1522 | Domain has pure hunk model; `src/git/` re-exports 11 symbols *and* defines its own git2-backed `ResolutionBuffer`. |
| `diff.rs`/`status.rs`/`diffstat.rs`/`checklist.rs` | small | small | **Clean splits** — `pub use kagi_domain::*` + git2 impl. These are the exemplars. |

A hunk-parsing bug may need fixing in two places. Reading the code, you must
constantly check which file owns which function.

## Decision

### 1. Rename `src/git/history.rs` → `src/git/file_history.rs`

The two `history.rs` files describe different concepts. The domain one
(`OperationKind`, `HistoryEntry`, `OperationHistory`) is the operation-log
history; the `src/git/` one is file-history (git log of a single path).
Rename the `src/git/` file to match its actual responsibility. Update
`src/git/mod.rs`.

### 2. Document the shim boundary in each `src/git/<x>.rs`

Every `src/git/<x>.rs` that re-exports from `kagi-domain` gets a header block
making the boundary explicit:

```rust
//! File-history git2 backend.
//!
//! Re-exports the pure types from `kagi_domain::history` and adds the
//! git2-backed read functions (`collect_file_history`, etc.).
//! Pure parsing/logic lives in `kagi_domain`; this file is git2 glue only.
```

The exemplars (`diff.rs`, `status.rs`, `diffstat.rs`, `checklist.rs`) already
follow this shape — replicate it.

### 3. Move remaining pure logic to `kagi-domain` (where clearly pure)

Audit `src/git/message_gen.rs` and `src/git/resolution.rs` for functions that
take no `&Repository`/`git2::` types and have no I/O. Move those to
`kagi-domain`. Keep only the git2-backed glue in `src/git/`.

**Target:** each `src/git/<x>.rs` is visibly smaller than its domain twin and
contains no pure logic — only git2-backed reads/writes delegating to
domain functions for parsing/rules.

### 4. Collapse the 18 `#[allow(unused_imports)]` re-export blocks in `src/git/mod.rs`

Audit each `pub use` in `src/git/mod.rs:30-109`. For each re-exported symbol,
grep `src/` + `tests/` for consumption. Drop unused re-exports; remove the
`#[allow]`. The migration has been ongoing long enough that some are stale.

## Consequences

- The filename collision (`history.rs` × 2) is resolved — reading the code no
  longer requires checking which concept a given `history::Foo` refers to.
- The shim/impl boundary is documented at the top of each file — "which
  function lives where" is answerable without cross-referencing.
- Double-maintenance surface shrinks toward the exemplar pattern.
- Type-layer cleanliness is preserved (the `CommitId`/`Head`/`Commit`
  re-exports are already single-definition, codebase-review §7 Finding 21 —
  this ADR doesn't touch those).

## Rollout

Mechanical, low-risk:
1. Rename `src/git/history.rs` → `file_history.rs`; update `mod.rs`. (One commit.)
2. Add header blocks to `src/git/{message_gen,resolution,diff,status,diffstat,
   checklist,file_history}.rs`. (One commit.)
3. Move clearly-pure functions from `src/git/{message_gen,resolution}.rs` to
   `kagi-domain`. (One commit per file; each `cargo test --workspace` green.)
4. Audit + trim `src/git/mod.rs` re-exports. (One commit.)

## What this does NOT change

- The crate split itself (S6 — moving `src/ui` → `crates/kagi-ui`) is
  deferred to Phase 5.
- No behavior change; pure refactoring.
