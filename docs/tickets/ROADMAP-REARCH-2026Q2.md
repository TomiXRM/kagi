# Roadmap: 2026 Q2 Rearch — Safety Pipeline + Performance + Architecture Foundation

> PM-owned roadmap for the one-day aggressive refactor sprint. Derived from
> `/docs/codebase-review.md`, `/docs/refactor-plan.md`,
> `/docs/git-safety-checklist.md`, `/docs/performance-review.md`.
>
> **Operating mode:** ADR + ticket driven. Every structural change has an ADR
> written *before* the code. Every step lands on the `rearch/safety-pipeline-and-foundation`
> branch (draft PR) and is cross-reviewed (by a separate Claude pass) before
> being merged toward `main`.
>
> **Guardrails (non-negotiable):**
> - `cargo test --workspace` green at every commit.
> - `cargo fmt --all` before every push.
> - No new `git2::` in `src/ui/` (CI gate).
> - No `reset --hard`, `push --force`, `git clean`, `--force-with-lease`, `unsafe`.
> - The existing `[kagi] …` log contract lines are a test contract — do not
>   reword existing lines; add new ones in the established format for new
>   coverage.
> - UI-behavior changes need a human/primary session to eyeball; build+tests
>   green is necessary but not sufficient.

## Phases

### Phase 0 — Dead-code sweep (zero-risk, do first)
Scope the surface down so later diffs stay readable.

| Ticket | Title | Files | Status |
|---|---|---|---|
| T-REARCH-001 | Delete dead `Backend::repo()` + redundant dev-dep `tempfile` + dead modal accessors + dead render/graph helpers | backend.rs, modal_state.rs, commands.rs, render.rs, graph_view.rs, modals.rs, Cargo.toml | todo |

### Phase 1 — Safety pipeline (the thesis; highest priority)
Make `plan → confirm → preflight → execute → verify → oplog` a backend guarantee.

| Ticket | Title | ADR | Status |
|---|---|---|---|
| T-REARCH-010 | Enforce pipeline in `Backend::run` (preflight → execute → verify → oplog); execute_* take `&OperationPlan` | ADR-0104 | todo |
| T-REARCH-011 | Route UI operations + headless `KAGI_*` hooks through `Backend::run` | ADR-0104 | todo |
| T-REARCH-012 | Block merge on dirty working tree (mirror cherry-pick rule) | ADR-0105 | todo |
| T-REARCH-013 | Atomic `stage_conflict_resolution` (temp-write-then-rename) | ADR-0106 | todo |
| T-REARCH-014 | Two-stage confirm on Discard (port amend's `confirm_armed`) | — | todo |
| T-REARCH-015 | Stash pop/drop take `&OperationPlan` + `preflight_check_stash` | (ADR-0104) | todo |

### Phase 2 — Architecture foundation
Unblocks perf (Phase 3) and the larger structural moves (Phase 5).

| Ticket | Title | ADR | Status |
|---|---|---|---|
| T-REARCH-020 | Introduce `RepoSession` owning the `Backend` (collapse 132 `Backend::open` sites) | ADR-0107 | todo |
| T-REARCH-021 | Finish `kagi-domain` extraction; rename `src/git/history.rs` → `file_history.rs` | ADR-0108 | todo |

### Phase 3 — Performance
Depends on Phase 1 (off-thread snapshot needs pipeline) and Phase 2 (RepoSession).

| Ticket | Title | ADR | Status |
|---|---|---|---|
| T-REARCH-030 | Move `reload_external` snapshot off the UI thread | — | todo |
| T-REARCH-031 | Per-file diff content cache by `(commit_oid, path)` | — | todo |
| T-REARCH-032 | Tree-sitter highlight off-thread (render text first) | — | todo |
| T-REARCH-033 | Render-path clone elimination (Arc-wrap views; drop `CommitRow::clone`; cache avatar color) | — | todo |
| T-REARCH-034 | Graph layout cache by `(head_oid, commit_count)` + pre-baked paths | — | todo |
| T-REARCH-035 | Single global auto-fetch ticker per remote-URL | — | todo |

### Phase 4 — UX (stretch for the day)
Only if Phases 1–3 land with time remaining.

| Ticket | Title | Status |
|---|---|---|
| T-REARCH-040 | Persistent error toasts (exclude `ToastKind::Error` from auto-dismiss/cap) | todo |
| T-REARCH-041 | Oplog "Restore" action for discard + delete-branch | todo |

### Phase 5 — Large structural (explicitly DEFERRED today)
Entity decomposition, worker thread, view-model layer, crate split, headless
retirement. These are tracked here for completeness but **will not be started
in this sprint** — they require Phase 1+2 to be stable and reviewed first.

## Sprint order

1. **Setup:** worktree + branch + commit review docs (5 min).
2. **Phase 0** (parallel SubAgent, ~30 min).
3. **Phase 1** — T-REARCH-010/011 first (the keystone), then 012/013/014/015
   in parallel. Cross-review after 010/011.
4. **Phase 2** — T-REARCH-020 (RepoSession) once 011 lands; 021 can parallel.
5. **Phase 3** — parallel after 020 lands.
6. **Phase 4** — if time permits.
7. **Final:** full `cargo fmt/clippy/test`, Claude cross-review, PR ready for review.

## Definition of Done (for the draft PR)

- [ ] All Phase 1 tickets done + reviewed (safety pipeline is a backend guarantee).
- [ ] Phase 0 cleanup merged in.
- [ ] At least Phase 2 T-REARCH-020 (RepoSession) done OR explicitly deferred
      with rationale.
- [ ] `cargo test --workspace` green; `cargo fmt --check` clean; clippy no new warnings.
- [ ] ADRs 0104–0108 written and linked from tickets.
- [ ] Cross-review notes from Claude addressed or triaged.
- [ ] CHANGELOG entry.
