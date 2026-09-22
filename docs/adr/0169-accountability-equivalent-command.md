# ADR-0169 — Accountability UX: equivalent git command + `GIT_ADVICE=0`

Status: Accepted
Issue: #353 (parent #359)
Date: 2026-09-04

## Context

Kagi executes via **libgit2**, not the `git` CLI, so two things git users expect
are missing:

1. git's own `advice.*` output (≈40 items in `Documentation/config/advice.adoc`)
   never appears — the human-facing explanation only exists if kagi writes it.
2. There is no "command that was run" to show, because none is. But an
   **equivalent** command can be shown honestly — the Sublime Merge "Real Git"
   idea, adapted: *"this is equivalent to `<cmd>`"*, never *"runs `<cmd>`"*.

## Decision (this slice)

### 1. `OperationPlan.equivalent_command: Option<String>`

A new pure field on `OperationPlan` (`crates/kagi-domain/src/plan.rs`). It is
built as a plain string from plan data in the per-feature `ops/<feature>.rs`
builders — kagi-domain itself stays git2-free; it only carries the `Option`.

Emitted (`Some`) for the starter subset where the CLI mapping is genuinely
faithful — the destructive / network / history ops:

| Op | Equivalent | Where |
|---|---|---|
| push | `git push [-u] <remote> <branch>` | `ops/push.rs` |
| force-with-lease push | `git push --force-with-lease=<branch>:<lease> <remote> <branch>` | `ops/force_lease.rs` |
| branch delete | `git branch -d <name>` (kagi blocks unmerged deletes, so `-d`) | `ops/branch.rs` |
| checkout (branch) | `git checkout <branch>` | `ops/checkout.rs` |
| reset current → commit | `git reset --soft <sha>` (ref-only; never `--hard`) | `ops/reset.rs` |

Left `None` (honesty over coverage, issue §5):

- **discard** — libgit2's `checkout_index` diverges from `git checkout --` /
  `git restore` on eol normalization. A wrong equivalent is worse than none.
- every other plan kind (commit, merge, rebase, pull, stash, cherry-pick,
  revert, tag, worktree, …) — not yet vetted for faithfulness.

The plan modal renders one muted line via `Msg::PlanEquivalentTo`:
EN `"This is equivalent to \`{}\`"`, JA `"この操作は \`{}\` に相当します"`.
The wording is deliberately **"equivalent to" / "相当"**, never
**"runs" / "実行"** — asserted by a test — because kagi does not run the CLI.

### 2. `GIT_ADVICE=0` on subprocess `git` / `gh`

`crates/kagi-git/src/cli.rs` gained `git_command()` and `gh_command()` builders
that set `GIT_ADVICE=0` (alongside the existing non-interactive env). `run_git`
now goes through `git_command`; the ~12 `Command::new("gh")` sites across
`github.rs` / `github_merge.rs` / `ruleset.rs` now go through `gh_command()`.
This stops git's advice from doubling up with kagi's own UI guidance.

**Embedded terminal (`src/ui/terminal.rs`) deliberately left alone**: it runs
the user's interactive shell, not a kagi-driven git call. Forcing
`GIT_ADVICE=0` there would suppress advice in the user's own CLI, where it is
helpful (issue §5 open question — answered "no" for the interactive shell).

## Advice Msg catalog slice (2026-09-23)

The existing `OperationPlan` advice is typed `PlanNote` data, with EN rendered
by `kagi-domain::plan_note` and JA by `kagi-ui-core::i18n::plan` (ADR-0129).
These notes were already translated; this slice makes ten common guidance
kinds explicit `Msg::Advice…` keys and routes their existing JA consumers
through those keys. It does not introduce new prose or change severity.

Selection uses production constructor reuse, then everyday operation visibility
and actionable guidance. These are **static source counts, not usage telemetry**;
test constructors, renderers and declarations are excluded. The seven
single-site kinds below are a product-priority selection, not a measured ranking.

| Msg key (all prefixed `Advice`) | Existing note | Production sites |
|---|---|---:|
| `UntrackedRemain(ctx)` | `CommonNote::UntrackedRemain` | 8 |
| `SuggestStashPush` | `CommonNote::SuggestStashPush` | 3 |
| `NoForceUsed(punct)` | `PushNote::NoForceUsed` | 2 |
| `WillDetachHead` | `CheckoutNote::WillDetachHead` | 1 |
| `RecommendCreateBranchHereFirst` | `CheckoutNote::RecommendCreateBranchHereFirst` | 1 |
| `DirtyStashFirst` | `CommonNote::DirtyStashFirst` | 1 |
| `PullAutoStash` | `PullNote::AutoStash` | 1 |
| `DivergedSwitchOnly` | `SwitchNote::DivergedSwitchOnly` | 1 |
| `DeleteUnmerged` | `BranchNote::DeleteUnmerged` | 1 |
| `UntrackedExcluded` | `StashNote::UntrackedExcluded` | 1 |

The ten kinds cover 20 production constructor sites. Untracked context and push
punctuation preserve 17 English forms rather than flattening distinct messages.
`DirtyRollbackHint` is not selected: its sole helper caller filters it out;
the actual dirty Pull UI uses `AutoStash`, not the superseded `DirtyPullGuard`.

`advice_template_en!` in the existing domain note module owns the selected
English literals. Both the original `message_en()` formatters and the static
`Msg` templates use them; EN UI/CLI/oplog still use `message_en()` with unchanged
bytes. Literal macro expansion preserves Rust's format-argument checks without
an English runtime template engine. JA templates stay in their existing
per-family files. `Msg::t_for(Ja)` keeps explicit JA renderers independent of
the process language. JA argument insertion is single-pass: identifiers
containing `{}` and literal Git syntax such as `stash@{0}` are not reinterpreted.

Plan variants, warning/blocker classification, execution, and equivalent
commands are unchanged. The all-context EN/JA smoke, identifier/locale edge
regressions, and existing native plan scenarios exercise the cutover.

## Remaining advice Msg catalogue (#353)

The follow-up inventories all 24 families and all 224 `PlanNote` payload
variants, rather than selecting another commonly used subset. A note is advice
when it is an operation warning/effect/safety explanation or contains a next
action in either EN or JA, including such guidance inside a blocker. Plain
identity/no-op/refusal text without guidance and opaque error payloads remain
outside this migration. Blocker prose and severity are not rewritten.

154 additional Note variants have newly keyed guidance: 161 full-note/context
keys plus three auxiliary keys make **164 new Msg keys**, or **174 Advice Msg
kinds** with the prior ten. Conditional templates account for the difference
between variant and key counts. The auxiliary keys are
`AdvicePullRestoreConflictMore`, `AdviceStashConflictUnknownFiles`, and
`AdviceGithubLocalBranchNotDeletedDeletionUnauthorized`. Each new key has EN
and JA arms; all existing ten keys and their contextual forms remain intact.

### Complete family disposition

The source links identify each family's enum and its canonical EN consumers.
Every variant is accounted for by the prior-key table above, the newly keyed
count, or the explicit outside-advice list below.

| Family | Note variants | Prior advice variants | Newly keyed variants | New Msg keys | Outside advice |
|---|---:|---:|---:|---:|---:|
| [`common`](../../crates/kagi-domain/src/plan_note/common.rs) | 15 | 3 | 7 | 7 | 5 |
| [`commit`](../../crates/kagi-domain/src/plan_note/commit.rs) | 4 | 0 | 3 | 3 | 1 |
| [`discard`](../../crates/kagi-domain/src/plan_note/discard.rs) | 5 | 0 | 3 | 3 | 2 |
| [`snapshot`](../../crates/kagi-domain/src/plan_note/snapshot.rs) | 3 | 0 | 2 | 2 | 1 |
| [`ruleset`](../../crates/kagi-domain/src/plan_note/ruleset.rs) | 13 | 0 | 11 | 11 | 2 |
| [`checklist`](../../crates/kagi-domain/src/plan_note/checklist.rs) | 4 | 0 | 4 | 4 | 0 |
| [`merge`](../../crates/kagi-domain/src/plan_note/merge.rs) | 17 | 0 | 12 | 12 | 5 |
| [`rebase`](../../crates/kagi-domain/src/plan_note/rebase.rs) | 5 | 0 | 2 | 2 | 3 |
| [`cherry_revert`](../../crates/kagi-domain/src/plan_note/cherry_revert.rs) | 6 | 0 | 2 | 3 | 4 |
| [`reset`](../../crates/kagi-domain/src/plan_note/reset.rs) | 5 | 0 | 3 | 3 | 2 |
| [`history`](../../crates/kagi-domain/src/plan_note/history.rs) | 13 | 0 | 8 | 9 | 5 |
| [`conflicts`](../../crates/kagi-domain/src/plan_note/conflicts.rs) | 15 | 0 | 14 | 14 | 1 |
| [`branch`](../../crates/kagi-domain/src/plan_note/branch.rs) | 12 | 1 | 9 | 9 | 2 |
| [`checkout`](../../crates/kagi-domain/src/plan_note/checkout.rs) | 7 | 2 | 3 | 3 | 2 |
| [`switch`](../../crates/kagi-domain/src/plan_note/switch.rs) | 8 | 1 | 3 | 3 | 4 |
| [`worktree`](../../crates/kagi-domain/src/plan_note/worktree.rs) | 21 | 0 | 14 | 17 | 7 |
| [`tag`](../../crates/kagi-domain/src/plan_note/tag.rs) | 6 | 0 | 3 | 4 | 3 |
| [`cleanup`](../../crates/kagi-domain/src/plan_note/cleanup.rs) | 6 | 0 | 5 | 5 | 1 |
| [`push`](../../crates/kagi-domain/src/plan_note/push.rs) | 6 | 1 | 3 | 3 | 2 |
| [`pull`](../../crates/kagi-domain/src/plan_note/pull.rs) | 13 | 1 | 10 | 11 | 2 |
| [`stash`](../../crates/kagi-domain/src/plan_note/stash.rs) | 12 | 1 | 9 | 10 | 2 |
| [`remote_branch`](../../crates/kagi-domain/src/plan_note/remote_branch.rs) | 2 | 0 | 1 | 1 | 1 |
| [`force_lease`](../../crates/kagi-domain/src/plan_note/force_lease.rs) | 4 | 0 | 3 | 3 | 1 |
| [`github`](../../crates/kagi-domain/src/plan_note/github.rs) | 22 | 0 | 20 | 22 | 2 |
| **Total** | **224** | **10** | **154** | **164** | **60** |

Two mixed payloads need context-level qualification:

- `TagNote::NameError`: `Empty` and `LeadingDash` are advice; `InvalidRef`
  and `Exists` remain plain validation refusals.
- `CommonNote::BranchNameErrorKeyed`: `CreateInvalidRef` has authored
  permitted-character guidance, so its shared domain Display/UI helper now use
  `AdviceCommonBranchInvalidRef`. Other payloads retain their existing Msg
  keys or plain validation formatters. `WorktreePathErrorKeyed` remains the
  existing validation delegate; it is not another Advice key.

| Family | Variants outside this advice migration |
|---|---|
| `common` | `HeadDetached`, `HeadUnborn`, `BranchMissing`, `GitErrorPassthrough`, `WorktreePathErrorKeyed` |
| `commit` | `EmptyMessage` |
| `discard` | `NothingSelected`, `NoUnstagedChanges` |
| `snapshot` | `SnapshotMissing` |
| `ruleset` | `UpdateBlocked`, `DeletionBlocked` |
| `checklist` | None |
| `merge` | `TargetIsCurrent`, `TargetIsHead`, `AlreadyContains`, `NoChanges`, `IntoAlreadyContains` |
| `rebase` | `DetachedHead`, `InvalidOnto`, `AlreadyUpToDate` |
| `cherry_revert` | `MergeCommitNeedsMainline`, `NothingToCherryPickHead`, `NoChanges`, `NotInCurrentBranch` |
| `reset` | `DetachedHead`, `CommitMissing` |
| `history` | `MergeCommitUnsupported`, `RootCommit`, `EmptyMessage`, `BranchNoTarget`, `BranchGone` |
| `conflicts` | `ChecklistBlocker` |
| `branch` | `CommitMissing`, `DeleteDetachedAtTip` |
| `checkout` | `AlreadyCurrent`, `CommitAlreadyHead` |
| `switch` | `LocalNameEmpty`, `LocalExists`, `NameEmpty`, `NoUpstreamToSwitch` |
| `worktree` | `BranchInOtherWorktree`, `AlreadyUnlocked`, `LockStateUnreadable`, `WorktreeMissing`, `RemoveMainRefused`, `AlreadyLocked`, `PruneNothing` |
| `tag` | `CommitMissing`, `NotFound`, `NoRemote` |
| `cleanup` | `NoSelection` |
| `push` | `NoUpstreamWithErr`, `AlreadyUpToDate` |
| `pull` | `NoUpstream`, `AlreadyUpToDate` |
| `stash` | `NothingToStash`, `IndexOutOfRange` |
| `remote_branch` | `NotFound` |
| `force_lease` | `NothingToPush` |
| `github` | `LocalBranchDeleted`, `LocalBranchAbsent` |

Outside-advice reasons are plain identity/existence/no-op or unsupported/input
refusals, except the explicitly opaque error wrappers
(`GitErrorPassthrough`, `NoUpstreamWithErr`, `PullNote::NoUpstream`,
`LockStateUnreadable`, `ChecklistBlocker`) and the existing keyed worktree-path
validation delegate. No guidance is inferred and no replacement prose is added
to turn an excluded refusal into advice.

Catalogue coverage is not a claim that every enum variant currently reaches a
confirmation card. `CommonNote::DirtyRollbackHint` is filtered from its only
producer; `BranchNote::DeleteRemovesPinningWorktree` is a dormant legacy note;
`RulesetNote::{LinearHistoryRequired, NonFastForward, ConstraintsUnknown}` have
no production plan consumer. Their existing renderers are catalogued without
adding producers. The excluded ruleset Update/Deletion variants likewise have
no producer. Local Pull replaces the backend's `DirtyPullGuard` with the
existing `AutoStash` note; the backend guard remains catalogued.

### Ownership and preservation

The original exported `advice_template_en!` keeps its API and single ownership
of English literals. Its growing data table is physically extracted to
`crates/kagi-domain/src/plan_note/advice.rs` (565 lines at this cutover), instead
of expanding the category dispatcher past the file-size target. `Msg` remains
the sole key/locale registry; the existing family files own JA constants and
argument order. No second catalogue type, runtime English template parser,
translation dependency, Note schema, or compatibility path is introduced.
The existing large Msg registry gains only the corresponding static entries;
its LOC ratchet is updated, not bypassed.

All text, commands, warning/blocker classifications, and producer conditions are
preserved. Nested typed notes still localize recursively. Dynamic identities,
errors, existing requirement strings, and structured worktree-step summaries
remain data; this extraction does not reparse or translate those payloads.
Existing localized fragments (dirty counts, file-list caps, operation labels)
retain their separators, omission rules, and language-specific order. Literal
Git braces require the runtime JA form `stash@{{}}`, rather than copying Rust's
`format!` escaping into the single-pass interpolator.

The paired source-runtime oracle exercised 395 constructed inputs in both
locales: 790 output rows / 351,562 bytes matched the pre-cutover domain,
localized, and explicitly JA renderers byte for byte. It reached all 164 new JA
keys and all 181 old/new contextual Advice forms. Inputs include empty/optional
and capped lists, every operation/direction and dirty-parts shape, nested PR
reasons, and Unicode identifiers containing literal `{}`. A permanent stash
identity regression guards literal `stash@{3}` plus an identifier containing
`{}`; the throwaway oracle is not added to the repository.

Native EN/JA acceptance confirmed Stash Push's new `UntrackedIncluded` text
together with the existing inspect/restore advice, without missing or mixed
prose. The no-remote Push fixture instead refuses **before planning** with the
English contract footer `Push: no upstream and no remote configured`; it does
not display a `NoUpstreamNoRemotes` advice modal. This was accepted with that
note covered by the paired renderer smoke above, not claimed as native-modal
coverage or a native Tier A runner scenario. The existing English stash-message
label is unchanged and outside this catalogue extraction.

### Upstream Git advice inventory

Inventory source: Git `Documentation/config/advice.adoc` at
[`6d6939a568d76120e77058e5959c3b82b4b90f1c`](https://github.com/git/git/blob/6d6939a568d76120e77058e5959c3b82b4b90f1c/Documentation/config/advice.adoc).
The snapshot has 43 named entries, including the aggregate `pushUpdateRejected`
setting. This is a mapping of Git's advice topics to Kagi-owned note templates,
not a claim that Kagi reproduces every Git stderr diagnostic. `No dedicated
mapping` means that topic has no authored `PlanNote` advice to migrate in this
catalogue; it does not mean that the underlying Git feature is unsupported.
Subprocess advice remains suppressed as described above; the interactive terminal
continues to show Git's own advice.

| Git advice entry | Kagi catalogue counterpart / disposition |
|---|---|
| `addEmbeddedRepo` | No dedicated mapping: embedded-repository staging diagnostic. |
| `addEmptyPathspec` | No dedicated mapping: CLI pathspec-omission hint. |
| `addIgnoredFile` | No dedicated mapping: ignored-path staging diagnostic. |
| `amWorkDir` | No dedicated mapping: `git am` patch-location hint. |
| `ambiguousFetchRefspec` | No dedicated mapping: ambiguous fetch-refspec diagnostic. |
| `checkoutAmbiguousRemoteBranchName` | No dedicated mapping: ambiguous CLI name lookup; the switch plan carries a selected remote ref. |
| `commitBeforeMerge` | `CommonDirtyBlocksOp`, `CommonDirtyRollbackHint`, and family-specific dirty-tree advice. The rollback hint is currently filtered from its only plan producer. |
| `detachedHead` | Existing `WillDetachHead` and `RecommendCreateBranchHereFirst`. |
| `diverging` | Existing `DivergedSwitchOnly`; `PullCannotFastForward`, `PullRemoteDiverged`. |
| `fetchRemoteHEADWarn` | No dedicated mapping: remote-HEAD-change diagnostic. |
| `fetchShowForcedUpdates` | No dedicated mapping: forced-update-check timing/configuration hint. |
| `forceDeleteBranch` | Existing `DeleteUnmerged`; `BranchDeleteSquashMerged`, branch/worktree refusal guidance. Kagi retains its own guarded-delete policy. |
| `ignoredHook` | No dedicated mapping: non-executable-hook diagnostic. |
| `implicitIdentity` | No dedicated mapping: Git's inferred-identity configuration hint. |
| `mergeConflict` | `MergeWillConflict`, `CommonMergeConflictWarning`, and stash/conflict-family resolution advice. |
| `nestedTag` | No dedicated mapping: recursively tagging a tag object. |
| `pushAlreadyExists` | `TagPushRejectedIfMoved` explains refusal when the remote tag already points elsewhere; not a reproduction of every server diagnostic. |
| `pushFetchFirst` | No dedicated missing-object diagnostic; existing `NoForceUsed` remains the general normal-push safety notice. |
| `pushNeedsForce` | No dedicated non-commit-object diagnostic; existing `NoForceUsed` and `TagPushRejectedIfMoved` describe Kagi's refusal policy. |
| `pushNonFFCurrent` | Existing `NoForceUsed`; `HistoryAmendDivergesFromRemote` explains the ordinary-push consequence of rewriting a published commit. |
| `pushNonFFMatching` | No dedicated mapping: matching-refspec CLI push diagnostic. |
| `pushRefNeedsUpdate` | `ForceLeaseNoUpstream`, `ForceLeaseLeaseValue`, `ForceLeaseRewritesRemoteHistory` explain the known-tip/lease contract; no separate Git stderr subtype parser. |
| `pushRepoLooksLikeRef` | No dedicated mapping: CLI remote/ref argument-shape hint. |
| `pushUnqualifiedRefname` | No dedicated mapping: ambiguous destination namespace hint. |
| `pushUpdateRejected` | Aggregate setting for other push advice, not an additional message. |
| `rebaseTodoError` | No dedicated mapping: edited rebase-todo syntax diagnostic. |
| `refSyntax` | `CommonBranchInvalidRef`, tag-name advice contexts, and existing typed validation keys/helpers. Plain validation refusals remain outside the advice migration. |
| `resetNoRefresh` | No dedicated mapping: index-refresh timing/CLI optimization hint. |
| `resolveConflict` | `CommonConflictedFiles`, `CommitConflictedFiles`, `ChecklistConflictMarkerFound`, and conflict-family resolve/reload guidance. |
| `rmHints` | No dedicated `git rm` retry mapping. Discard's separately owned safety guidance is keyed, without claiming equivalent command coverage. |
| `sequencerInUse` | `MergeOperationInProgress` retains its finish-or-abort instruction. |
| `skippedCherryPicks` | No dedicated mapping: automatic rebase-skip hint. Plain no-change notes are not advice. |
| `sparseIndexExpanded` | No dedicated mapping: sparse-index expansion diagnostic. |
| `statusAheadBehind` | No dedicated mapping: ahead/behind computation timing hint. |
| `statusHints` | Common dirty/untracked notes, `CommitNothingStaged`, and operation-specific next-action/effect advice provide Kagi's owned guidance rather than Git's status output. |
| `statusUoption` | No dedicated mapping: untracked-enumeration timing/CLI optimization hint. |
| `submoduleAlternateErrorStrategyDie` | No dedicated mapping: submodule alternate-error-strategy diagnostic. |
| `submoduleMergeConflict` | No dedicated submodule-specific advice mapping. General conflict guidance is not counted as that subtype. |
| `submodulesNotUpdated` | No dedicated mapping: missing submodule initialization hint. |
| `suggestDetachingHead` | Existing `WillDetachHead` and `RecommendCreateBranchHereFirst` explain the explicit checkout decision. |
| `updateSparsePath` | `CommonSparseExcludedPath` explains the absent-not-deleted distinction and widening sparse-checkout first. |
| `waitingForEditor` | No dedicated mapping: external-editor wait hint. |
| `worktreeAddOrphan` | No dedicated mapping: unborn-worktree creation hint. |

## Deferred (NOT in this slice)

- **Blocker wording rewrite** ("forbidden" → "next action") across all
  `PlanNote` blockers, plus optional action buttons.
- **Extending `equivalent_command` to the remaining plan kinds** — each needs a
  per-op faithfulness review before it can honestly emit.

These overlap the JP-wording work in **#376** and are tracked there / under the
parent #359.
