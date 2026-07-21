# ADR-0130: Force-with-lease Push (branch-menu "Force-with-lease push...")

- Status: Accepted
- Date: 2026-07-21

## Context

ADR-0009 (§2) deferred force-with-lease push ("後者は later 検討"). ADR-0040
案C designed the future "Advanced force-with-lease flow" in detail but left
it unimplemented pending a follow-up ADR. ADR-0055 similarly deferred
"Delete remote branch" to the same 案C isolation/staged-confirm family
(delivered in #198). This ADR implements the branch-menu "Force-with-lease
push..." item (Advanced / Dangerous group), closing out that deferral.

## Decision

- **Only** `--force-with-lease` — `--force` never appears as an executable
  code path anywhere in kagi (AGENTS.md invariant #3 already names `push
  --force`; this ADR is the record that `--force-with-lease` is the sole,
  deliberate exception, isolated to one file).
- Isolated to its own module (`crates/kagi-git/src/ops/force_lease.rs`),
  not `push.rs` — `push.rs`'s existing "force / --force / --force-with-lease
  are never used anywhere in this module" doc comment stays true and
  auditable by grepping that one file; the exception lives in exactly one
  place, named for what it is.
- **The lease value is never refreshed by an automatic fetch before
  pushing.** This is the entire safety property: the lease is the local
  record of `refs/remotes/<remote>/<branch>` as of whenever the user (or
  kagi's own auto-fetch) last looked. If someone else pushed since then,
  the remote rejects the push outright instead of silently overwriting
  unseen work. An automatic pre-push fetch would erase this protection —
  it would always lease against whatever is on the remote *right now*,
  which is indistinguishable from a blind `--force`. (ADR-0040 案C's design
  note "実行前にfetchまたはremote state確認を必ず行う" is satisfied by the
  plan modal's `LeaseValue` note, which shows the exact SHA being leased
  before the user confirms — not by an automatic fetch call.) Verified by
  `tests/force_lease_push_test.rs::test_execute_rejects_when_remote_moved_since_last_fetch`.
- **Two-stage armed confirm** (`confirm_armed`, mirrors `discard` /
  `delete-remote-branch` / `reset-current-to-head`), not ADR-0040 案C's
  original "type the branch name or 'force-with-lease' to continue"
  typed-confirmation design. The click-twice pattern was chosen to keep the
  UX consistent across every "Advanced / Dangerous" item added in this
  round (#198, #201, this ADR) rather than mixing confirmation styles
  within the same menu group; a typed-confirmation upgrade remains
  available as a future enhancement if the lighter pattern proves
  insufficient in practice.
- Scoped to the **current branch only** — the menu item is enabled only
  when right-clicking the currently checked-out branch's row
  (`force_lease_push_state`), since `plan_force_with_lease_push` /
  `execute_force_with_lease_push` always resolve `HEAD`, not the clicked
  row's `state.name`. (Plain `Push`/`Pull` support pushing/pulling a named
  non-current branch too; force-with-lease intentionally does not, to keep
  the blast radius to "the branch I'm looking at right now.")
- Recovery text gives the exact restore command
  (`git push --force-with-lease=<branch>:<new-sha> <remote>
  <previous-sha>:refs/heads/<branch>`), itself lease-protected against the
  just-completed push.

## Consequences

- ADR-0055's "Delete remote branch は MVP 外" line and ADR-0040 案C are both
  now fully delivered; no further "later" markers remain for either.
- `docs/adr/0009-toolbar-operation-policy.md`'s push row ("force /
  force-with-lease を実装しない") is superseded by this ADR for the
  force-with-lease half; plain force is still never implemented anywhere.
