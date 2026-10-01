# ADR-0171: Per-worktree port allocation + KAGI_* environment map

- Status: Accepted (backend allocation and terminal injection; remaining follow-ups below)
- Date: 2026-09-04
- Closes (partial): #342
- Depends on: #341 / ADR-0161 (worktree steps — the `command` step consumes the
  same `KAGI_*` vars), ADR-0035 (vendored `gpui-terminal`, the injection point)

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

The original PR landed the **deterministic, fully testable backend**:

- a pure port allocator + pure `KAGI_*` env-map builder (`kagi-domain`);
- persistence of assignments keyed by canonical worktree path (`kagi-git`);
- the two settings + typed accessors (`kagi-ui-core`);
- wiring: on worktree **create**, the block is computed and persisted.

The terminal follow-up now injects the existing five-variable payload at shell
spawn. The `nonconcurrent` run-mode UX and sidebar `http://localhost:<port>`
link remain **out of scope**.

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
`… exhausted …` line). Assignment is idempotent + lazy, so terminal spawn also
calls the existing `worktree_env` builder for worktrees that predate this feature.

`worktree_ports::terminal_env` resolves the actual worktree registry name,
main working tree and the existing branch-cleanup default-branch policy in
the Git layer. `src/ui/terminal.rs` supplies the typed port settings and applies
the returned variables to `CommandBuilder` before spawning; the cwd remains
the owning terminal session's repository path. All five values override inherited
`KAGI_*` values. A retained shell keeps its environment; a restarted shell recalls
the persisted block. Metadata failures use the existing terminal-start
failure/oplog path rather than spawning with a stale inherited port.

**Range exhaustion still starts the shell (#852).** `terminal_env` answers
`TerminalEnv::Exhausted { worktree, range, per }` instead of an error; the
terminal then starts with no `KAGI_*` var at all (never an out-of-range or
inherited port), logs `terminal: port block exhausted <path> (range
<start>-<end>, per <n>)`, and puts the reason in the footer and a toast (EN/JA),
naming `worktree.port_range` as the setting that widens the range. It is not an
operation, so nothing is recorded in the oplog. Refusing the whole shell over a
missing convenience variable left the user with no terminal at all. The default
range is unchanged.

## Alternatives considered

- **Bind sockets to reserve** — rejected for v1 (see above); revisit if false
  collisions are reported.
- **Persist inside the repo** (`.kagi/…`) — rejected; port blocks are
  machine-local, not shareable state, and would churn the working tree.

## Consequences

- **Repositories do not collide with each other.** There is one store
  (`$KAGI_LOG_DIR` or `~/.kagi/worktree_ports.json`) keyed by the canonical path of
  every worktree of every repository, and one range from the global settings;
  `allocate_block` treats every other stored entry as occupied. Two repositories
  therefore never receive the same block. (An earlier revision of this section
  claimed they could; it did not match the code — corrected by #852.)
- **The range is shared machine-wide, so it can run out.** The default
  `3000-3099` with 10 per worktree is 10 blocks in total. Main worktrees take a
  block too (`terminal_env` assigns for name `main`), and the bottom panel opens
  on the Terminal tab, so every repository or worktree that has shown a
  terminal holds one. Blocks are reclaimed only from worktrees whose directory
  is gone. Exhaustion no longer stops the terminal (see Wiring); widening
  `worktree.port_range` is the remedy.
- **Known limitations.** Changing `ports_per_worktree` between runs can make a
  new block overlap a pre-existing stored block (stored data records only the
  first port); the fix is to clear `worktree_ports.json`. Ports are numbers
  only — nothing is bound, so a process outside Kagi can still hold a handed-out
  port and Kagi does not detect it.

## Follow-ups (out of scope — tracked under #342 / parent #359)

1. **Terminal process-group handling** — environment injection and cwd are
   implemented. How running processes are treated on worktree removal and
   coordination with the #340 lock remain separate follow-ups.
2. ~~**`run_mode: "nonconcurrent"`**~~ — done (#859, ADR-0213) as the
   `worktree_run_mode` setting: one worktree of a repository at a time runs a
   terminal shell; a second is blocked with the reason.
3. ~~**Sidebar `http://localhost:<port>` link**~~ — done (#855): each WORKTREES
   row shows its stored block as `localhost:<port>` (click opens the browser).
   The snapshot reads the store; nothing is assigned by showing a row. A
   terminal start refreshes the current worktree's row at once.
4. **Configurable env-var name / bind-to-reserve** — if the `KAGI_PORT` convention
   or the numbers-only race proves insufficient.
