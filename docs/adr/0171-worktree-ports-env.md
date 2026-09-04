# ADR-0171: Per-worktree port allocation + KAGI_* environment map

- Status: Accepted (backend foundation only — see Scope / Follow-ups)
- Date: 2026-09-04
- Closes (partial): #342
- Depends on: #341 / ADR-0161 (worktree steps — the `command` step consumes the
  same `KAGI_*` vars), ADR-0035 (vendored `gpui-terminal`, the eventual injection point)

## Context

"Parallel development across worktrees" hits one practical wall: **port
collisions**. Three worktrees each running `npm run dev` all grab 3000.
Conductor (`CONDUCTOR_PORT`, reserves 10 ports) and Uzi (`portRange` + `$PORT`)
independently arrived at the same answer — reserve a consecutive block per
worktree and expose it via an env var — which is strong evidence this is the
real shape of the problem. Zed shows two env vars (`ZED_WORKTREE_ROOT` /
`ZED_MAIN_GIT_WORKTREE`) already cover most task scripts.

kagi already has an embedded terminal (ADR-0008 / 0035), so it can do this with
no external dependency: just allocate a block and inject the environment.

## Scope of this ADR / PR

This PR lands the **deterministic, fully testable backend** only:

- a pure port allocator + pure `KAGI_*` env-map builder (`kagi-domain`);
- persistence of assignments keyed by canonical worktree path (`kagi-git`);
- the two settings + typed accessors (`kagi-ui-core`);
- wiring: on worktree **create**, the block is computed and persisted.

Deliberately **out of scope** (see Follow-ups): injecting the vars into the
embedded terminal, the `nonconcurrent` run-mode UX, and the sidebar
`http://localhost:<port>` link.

## Decision

### Settings (PM-locked §5)

Two flat string settings (ADR-0091 on-disk shape unchanged):

```json
{
  "worktree.port_range": "3000-3099",
  "worktree.ports_per_worktree": "10"
}
```

Typed accessors live in `kagi-ui-core/src/settings.rs`:
`Settings::worktree_port_range() -> (u16, u16)` (default `(3000, 3099)`) and
`worktree_ports_per_worktree() -> u16` (default `10`; `0`/unparsable → default).

### Allocation is numbers-only (v1)

We **assign numbers, we do not bind sockets.** A worktree gets
`ports_per_worktree` consecutive ports; the block's first port is `KAGI_PORT`.
Because nothing is bound, a handed-out number **can still be taken by an
unrelated process** before the user's dev server grabs it. That race is accepted
for v1; bind-to-reserve is a follow-up. Rationale: binding to reserve holds
sockets for the app's lifetime, needs release/re-bind on every worktree churn,
and interacts badly with the dev server wanting the *same* port — a large amount
of machinery for a race that, in the single-user local case, essentially never
fires.

### Pure allocator (`kagi_domain::worktree_ports`)

`allocate_block(range, per, assigned: &BTreeMap<path, first_port>, target)`:

- **Idempotent**: if `target` is already in `assigned`, its stored first port is
  returned unchanged.
- Otherwise returns the lowest **aligned** free block (`start`, `start+per`,
  `start+2·per`, …) that fits in the range and overlaps no other block — giving
  the `3000 / 3010 / 3020` layout the issue shows, and reusing a hole freed by a
  removed sibling.
- Returns `None` on **exhaustion** (every block taken, `per == 0`, or empty
  range). The caller surfaces this rather than handing out an out-of-range or
  overlapping port.

`env_map(...)` builds the five vars, in order:

| Variable | Value |
|---|---|
| `KAGI_WORKTREE_PATH` | this worktree's absolute path |
| `KAGI_WORKTREE_NAME` | worktree name |
| `KAGI_MAIN_WORKTREE` | main worktree's absolute path |
| `KAGI_DEFAULT_BRANCH` | default branch name |
| `KAGI_PORT` | the block's first port |

Both functions are pure (no I/O), so the allocation logic and the env contract
are unit-tested in `kagi-domain`.

### Persistence (`kagi_git::worktree_ports`)

Assignments are persisted so a worktree keeps the **same** block across kagi
restarts and across sibling churn. Stored as `{ canonical path → first port }`
JSON in `worktree_ports.json`, resolved `$KAGI_LOG_DIR` → `$HOME/.kagi` —
exactly like the oplog and the ADR-0161 worktree-trust store. `assign_block`
recalls an existing block or allocates + persists a new one; `worktree_env`
combines assignment with the pure env-map for the eventual terminal injection.

Keys are canonicalized best-effort (collapses symlinks / `..`, e.g. macOS
`/var` → `/private/var`); a not-yet-created path falls back to its lexical form.

### Wiring

On successful worktree **create** (`create_worktree_blocking`), the resolved
worktree path is assigned + persisted immediately, logged via the `klog!`
contract channel (`worktree-port: assigned <path> → <port>`, or an
`… exhausted …` line). Assignment is idempotent + lazy, so the deferred terminal
PR can call `worktree_env` at spawn time for worktrees that predate this feature.

## Alternatives considered

- **Bind sockets to reserve** — rejected for v1 (see above); revisit if false
  collisions are reported.
- **Persist inside the repo** (`.kagi/…`) — rejected; port blocks are
  machine-local, not shareable state, and would churn the working tree.

## Consequences

- Cross-repository collision (two repos both drawing from `3000-3099`) is **not**
  solved here — the store is global and first-come; two repos can hand out the
  same numbers. Documented as a known limitation (issue §5).
- Changing `ports_per_worktree` between runs can make a new block overlap a
  pre-existing stored block (stored data records only the first port). Acceptable
  and rare; the fix is to clear `worktree_ports.json`.

## Follow-ups (out of scope — tracked under #342 / parent #359)

1. **Terminal injection** — inject the `KAGI_*` map + cwd into the embedded
   `gpui-terminal` (ADR-0035). §5's process-group handling is still uninvestigated
   (how running processes are treated on worktree removal, coordinating with the
   #340 lock).
2. **`run_mode: "nonconcurrent"`** — the "correctly give up on parallelism" escape
   hatch for projects with a single shared DB / fixed callback URL; blocking or
   warning on a second launch is a UX decision left open.
3. **Sidebar `http://localhost:<port>` link** (click-to-open) — GUI change,
   needs human eyeballing.
4. **Configurable env-var name / bind-to-reserve** — if the `KAGI_PORT` convention
   or the numbers-only race proves insufficient.
