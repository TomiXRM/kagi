# ADR-0146: Hardening the git CLI against a hostile repository

- Status: Accepted
- Date: 2026-09-02
- Closes: #290, #291, #647, #649 (owner gate deferred to a follow-up)

## Context

kagi runs the system `git` binary for the operations libgit2 cannot do
(fetch/push/rebase/status-with-fsmonitor — ADR-0009 §3). Every such call goes
through one function, `run_git` (crates/kagi-git/src/cli.rs).

That function inherited the opened repository's own `.git/config` unfiltered.
Several git config keys are *executed*: `core.fsmonitor`, `core.sshCommand`,
`credential.helper`, `core.hooksPath`. A repository is data you are handed, not
code you audited — but opening one and waiting was enough to run its config's
commands, because auto-fetch is on by default and fires a silent fetch 180s
after open (#290). Separately, git subcommands took repo-supplied remote/ref
names positionally with no `--` separator, so a remote named
`--upload-pack=<cmd>` was executed as a flag (#291). Both are remote code
execution from merely opening a repo. libgit2 honours none of these keys, so
the git2 paths were already safe; only the CLI path was exposed.

## Decision

All hardening lives in `run_git`, so every present and future caller is covered
by construction.

### Config / environment (#290)

Prepended to every invocation, before the caller's subcommand:
`--no-pager -c core.fsmonitor= -c core.hooksPath=/dev/null -c core.askPass=
-c protocol.allow=user`, plus `GIT_ASKPASS=/bin/false` in the environment.

Two keys are neutralised **only when the repo-local or worktree config sets
them** (`core.sshCommand`, `credential.helper` / `credential.<url>.helper`),
not blanket — this is a deliberate deviation from #290's prescription, forced
by measurement:

- **`credential.helper=` blanket, and `GIT_CONFIG_NOSYSTEM=1`, were rejected.**
  On stock macOS the *system* gitconfig ships `credential.helper = osxkeychain`
  (verified: `/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig`
  on the dev machine). Emptying the helper list or dropping the system config
  breaks every HTTPS remote — the exact capability `run_git` exists to provide.
  The system config needs root to write and is outside the threat model; the
  attacker controls the *repo-local* config. So the reset is scoped to a
  helper the repo itself introduced.
- **`core.sshCommand=ssh`** is applied on the same repo-local condition, so a
  user's global `ssh -i …` survives while a repo-local sshCommand is neutered.

`protocol.allow=user` is applied unconditionally: it blocks `ext::` (the
CVE-2018-17456 class) while local-path / `file://` / normal remotes — all that
the app and its tests use at top level — still work. It will block a
submodule-triggered fetch over an exotic transport, which is the hardening
working as intended.

### Dynamic executable keys (#647, #649)

Attributes can select arbitrary merge, filter, and diff driver names. For
example, `f.txt merge=evil` selects `merge.evil.driver`, while
`f.txt filter=evil diff=evil` selects the corresponding `filter.evil.*` and
`diff.evil.*` settings. The names are not fixed allowlist values, and the
configuration values are executable commands.

Before every `run_git` child starts, Kagi opens the repository config and scans
the **local** and **worktree** levels once. It prepends an override for every
matching key:

- `merge.<name>.driver=`
- `filter.<name>.clean=`, `.smudge=`, and `.process=`
- `filter.<name>.required=false`
- `diff.<name>.command=` and `.textconv=`

The original key spelling is retained in the override. Kagi does not parse
`.gitattributes`: an attribute that names a driver with no matching config
definition has no executable command, while the config scan finds every
executable definition irrespective of its attribute spelling. A setting
defined only in global or system config remains available; the untrusted
repository did not define it.

Config inspection is a security precondition. An absent local/worktree config
is normal, but any other discovery, read, or enumeration failure aborts
`run_git` before it starts a child. Continuing after a failed scan would turn a
malformed or unreadable attacker-controlled config into a hardening bypass.

This establishes the general rule: **when an executable setting has a
repository-chosen key name, scan the untrusted config levels once; never rely
on a fixed key list or operation-specific scan.** Stash push consequently uses
the same shared scanner as rebase, diff, and every other current or future
`run_git` caller.

Rebase behavior was measured separately because Git's conversion path is not
obvious: a repository-selected filter process still executes during rebase
when `merge.renormalize=false`. Setting that key alone does not stop execution.
Kagi therefore neutralises the executable filter keys themselves and does
**not** override `merge.renormalize`; legitimate global/system behavior for
that non-executable setting is preserved.

If rebase then reports unstaged changes, the backend retains a typed
`RebaseCannotStartFiltersDisabled` error and the UI explains the safety
trade-off in the selected language, including the option to run `git rebase`
in a terminal when the repository's filters are trusted. This follows the
stash-push #623 precedent: preserve security-relevant identity as a type until
the presentation boundary rather than asking UI or oplog code to infer it from
human-readable error prose.

### Argument injection (#291) — two independent layers

1. **A `--` separator** at every site where git accepts one
   (`fetch/push/ls-remote/rebase`, positions verified against git 2.50.1).
   This makes injection *structurally unrepresentable*: nothing after `--`
   is read as a flag, regardless of validation.
2. **A leading-dash validator** (`is_flag_like` / `check_operand`, the single
   predicate ops/branch.rs already used for new branches, now shared) applied
   by callers to the untrusted *name* values. This covers what `--` cannot: a
   name interpolated *into* a flag value (`--force-with-lease=<branch>:<oid>`),
   names accumulated into a `Vec` before the operand list (branch_cleanup), and
   it turns the attack into a clean `GitError` instead of a git parse failure.

The validator is deliberately NOT inside `run_git`: a blanket "reject args
starting with `-`" there would reject the legitimate flags callers pass
(`--prune`, `-u`, `--force-with-lease=…`).

## Consequences

- Opening a hostile repo no longer executes its config's commands through the
  CLI, and a hostile remote/ref name is rejected or de-fanged.
- A repository that legitimately sets a local `credential.helper` has it reset
  rather than merged with the user's global helpers, so auth there fails loudly
  instead of silently — a ~10-line follow-up if such repos prove common
  (`ponytail:` note in cli.rs).
- Every security test carries a positive control (the same attack through bare
  `git` must fire first) and is mutation-verified; mutation E specifically
  fails if anyone later "corrects" the code to #290's literal text.
- **Deferred: the owner / `safe.directory` gate (#290 item 4).** git 2.35.2+
  already enforces ownership on every CLI call, so `run_git` inherits it; the
  realistic attack (clone/download a hostile repo you then own) defeats an
  owner check anyway; and a non-UI hard refusal would brick legitimate
  shared/root-owned repos with no way to grant an exception. Filed separately.
- Supersedes the CLI half of ADR-0009's "inherit the repo's git config"
  assumption.
