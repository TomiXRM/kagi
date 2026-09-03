# ADR-0161: Typed worktree steps (copy / symlink / command) + trust prompt

- Status: Accepted
- Date: 2026-09-03
- Closes: #341
- Depends on: #340 (worktree remove), ADR-0146 (hostile-repo `run_git` hardening)

## Context

Post-create / pre-remove hooks are the one worktree-manager feature every
surveyed tool has and kagi lacked (issue #341): copy `.env`, symlink IDE config,
`npm ci` after create; `docker compose down` / per-worktree DB cleanup before
remove ("delete the worktree, orphan the container" is a real incident).

The load-bearing precedent is **gwq v0.1.0**, whose committed `.gwq.toml`
`setup_commands` were an arbitrary-code-execution vector: cloning a repo and
using it ran the repo author's commands. `.kagi/worktree.toml` is likewise
**committed → attacker-controlled data, not code we audited**. Any hook feature
without a trust prompt reintroduces that CVE.

## Decision

### Typed steps, not a command list

`.kagi/worktree.toml` holds `[[post_create]]` / `[[pre_remove]]` arrays of three
typed steps:

```toml
[[post_create]]
type = "copy"                     # trust NOT required
from = ".env.example"
to = ".env"
[[post_create]]
type = "symlink"                  # trust NOT required
from = ".claude"
to = ".claude"
[[post_create]]
type = "command"                  # trust REQUIRED
run = "npm ci"
```

Typing is what lets the plan say *exactly* what runs ("copy: … → …", "command
(needs trust): npm ci") instead of "runs 3 shell commands". `copy` and
`symlink` have closed side effects and never need trust; only `command` does.

### Layering

- **kagi-domain** (`worktree_steps.rs`) — the pure `WorktreeStep` enum, its
  trust classification, per-type enumeration, and `escape_control_bytes`. No
  toml, no hashing (the crate stays dependency-free).
- **kagi-git** (`ops/worktree_steps.rs`) — TOML parse (`toml`), SHA-256
  (`sha2`/`hex`), the trust store, and the executor. `toml`/`sha2`/`hex` were
  already in `Cargo.lock`, so no new download.

### Trust granularity — repository-level, content-keyed (PM-locked §5)

The trust store `trusted_worktree_configs.json` (next to the oplog:
`$KAGI_LOG_DIR` then `$HOME/.kagi`) keys each entry by
`(canonical .kagi/worktree.toml path, SHA-256 of its bytes)`. It is distinct
from repo-open trust. **Editing the config moves the SHA, so trust no longer
matches and the plan re-prompts.** Per-step trust was rejected as too noisy.

### The plan-confirm modal *is* the trust prompt

Rather than a second modal, the existing create / remove **plan-confirm** modal
carries a `PostCreateSteps` / `PreRemoveSteps` note that enumerates every step
(command text run through `escape_control_bytes`, so a committed config cannot
spoof the display with control bytes / newlines) and, when a command step is
present in an untrusted config, an explicit "⚠ Confirming TRUSTS this config to
run the command step(s) above" line. Confirming that plan is the informed
consent; the confirm handler then records repository-level trust and executes.
This deviates from the ticket's "add an `ActiveModal` variant" wording, chosen
deliberately: the plan modal already shows the exact escaped commands and
requires an explicit confirm, so a parallel modal would add friction and code
without adding consent. A dedicated modal remains an easy follow-up if a
two-step "trust, *then* confirm" gesture is wanted.

### Command execution environment (PM-locked §5)

`command` runs through an argv array via `std::process::Command` — **never a
shell** (that is the gwq attack path). The process PATH already carries the
login-shell PATH (`shell_env.rs`), so `npm` resolves. The child gets a hardened
git environment — `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`,
`GIT_TERMINAL_PROMPT=0` — applying ADR-0146's lesson so a hostile committed
config cannot poison the child's git. Execution is async / off the GUI thread
with a 600s timeout that **kills the child** on expiry (reusing `cli.rs`'s
`wait_or_kill`, avoiding #294's leak). Argv is a whitespace split (no shell
means no quoting/expansion); quoted-arg support is a `ponytail:` follow-up.

### Safety rules

- **symlink** follows worktree-link's four rules: a directory links as one link
  (symlinks never recurse); the target is absolute (canonicalized); any path
  containing `.git` is refused; an existing destination is **never overwritten**
  (asserted with `symlink_metadata`, which also catches a dangling link).
- **pre_remove is a precondition of deletion.** A failed, untrusted, or
  headless-blocked `command` returns `Err` *before* any destructive step, so the
  worktree survives — matching kagi's preflight ethos and the phantom /
  vscode-extension convergence. There is **no `--force` escape hatch**.
- **Headless never runs a `command`** (asserted). The executor refuses when a
  `KAGI_*` headless marker is set. (`KAGI_LOG_DIR` is excluded from that set: it
  is only store/test isolation, and gating on it would make the trusted-command
  path untestable.)
- **copy** also never overwrites; `post_create` is best-effort (the worktree
  already exists, so a step failure never undoes it).

## Consequences

- A committed `.kagi/worktree.toml` is untrusted by default; its command steps
  never run until a human confirms a plan that visibly lists them, and any edit
  re-prompts. copy/symlink work with no prompt.
- Removing a worktree whose `pre_remove` cleanup fails / is untrusted keeps the
  worktree instead of orphaning its resources.
- Acceptance §6 is covered by `crates/kagi-git/tests/worktree_steps_test.rs`
  (each assertion mutation-verified) plus unit tests in both crates.
- **GUI-unverified:** the plan modal now renders the step enumeration and the
  trust line, and the confirm handlers grant trust — this needs a human to
  eyeball in the running app (subagents cannot exercise the GUI).
