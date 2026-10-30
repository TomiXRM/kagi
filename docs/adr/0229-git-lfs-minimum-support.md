# ADR-0229: Git LFS の最小対応を既存の Git 操作境界に載せる

- Status: Draft — backend implementation under verification
- Date: 2026-10-31
- Related: ADR-0146, ADR-0193, ADR-0194, ADR-0215; `docs/rearch/architecture.md`

## Context and source of truth

The first consumer is an existing repository with a **nested** `.gitattributes`
that assigns `filter=lfs diff=lfs merge=lfs -text` to CAD binaries while leaving
nearby text files and binaries outside that directory in ordinary Git. The
repository has a custom pre-push hook that calls Git LFS, and some deployments
deliberately use `GIT_LFS_SKIP_SMUDGE=1`. Its actual content is private; committed
tests use generated fixtures with the same attribute and pointer semantics.

Git LFS stores a small pointer in the Git index/tree and the content in a separate
local/remote object store. A Kagi/libgit2 status read can report a clean LFS file
as modified after only its mtime changes. `index.add_path` would stage raw bytes,
and libgit2 checkout writes pointers without smudging. Kagi currently blocks LFS
Stage and tracked Discard, but not every worktree writer. `run_git` disables hooks,
so Git push cannot rely on the repository's LFS pre-push hook. In an isolated
fixture, a push with that setting moved a remote ref containing an LFS pointer
while transferring zero LFS objects; an ordinary later `git lfs push` skipped
the already-pushed range. The initial release prevents this for its own new
pushes, but does not repair a pre-existing broken remote. See the Git LFS
spec and ADR-0194's filter amendment.

The canonical comparison is the Git LFS CLI with the same fixture: status,
index/tree pointer, worktree bytes, local LFS store, and remote LFS store. The
first comparison happens after backend fixture tests, before any UI change.

## Competitor implementation check (2026-10-31)

The following is a source-code comparison, not an assertion that either client
passes Kagi's safety policy. Both repositories were cloned at the pinned heads.

| Client | Backend evidence | Frontend evidence | Implication for Kagi |
|---|---|---|---|
| GitHub Desktop (`952326e`) | [`lfs.ts`](https://github.com/desktop/desktop/blob/952326e873ad4c509e82cef0cc70f7272b3ec876/app/src/lib/git/lfs.ts) checks `git check-attr filter`, installs Git LFS filters/hooks; [`push.ts`](https://github.com/desktop/desktop/blob/952326e873ad4c509e82cef0cc70f7272b3ec876/app/src/lib/git/push.ts) shells out to Git and intercepts `pre-push` hook output with LFS progress. | [`sidebar.tsx`](https://github.com/desktop/desktop/blob/952326e873ad4c509e82cef0cc70f7272b3ec876/app/src/ui/changes/sidebar.tsx) checks oversized files before Commit; [`initialize-lfs.tsx`](https://github.com/desktop/desktop/blob/952326e873ad4c509e82cef0cc70f7272b3ec876/app/src/ui/lfs/initialize-lfs.tsx) offers a setup dialog. [GitHub's user guide](https://docs.github.com/en/desktop/configuring-and-customizing-github-desktop/about-git-large-file-storage-and-github-desktop) says LFS is bundled and tracking is configured on the command line. | Keep the existing file/commit/push surfaces. Kagi's hook-suppressed push must upload explicitly; it must not auto-install global filters or repo hooks. |
| GitButler (`fefe566`) | [`unified_diff.rs`](https://github.com/gitbutlerapp/gitbutler/blob/fefe5663b45e2d72a0a8ad1faabb9668f9444578/crates/but-core/src/unified_diff.rs) uses a `gix` conversion pipeline for Git-form content, including LFS manifests; [`hunks.rs`](https://github.com/gitbutlerapp/gitbutler/blob/fefe5663b45e2d72a0a8ad1faabb9668f9444578/crates/but-core/src/hunks.rs) retains long-running filters. [`hooks.rs`](https://github.com/gitbutlerapp/gitbutler/blob/fefe5663b45e2d72a0a8ad1faabb9668f9444578/crates/gitbutler-repo/src/hooks.rs) runs pre-push hooks explicitly. | The [GPUI client](https://github.com/gitbutlerapp/gitbutler/blob/fefe5663b45e2d72a0a8ad1faabb9668f9444578/crates/but-gpui/README.md) calls `but-api` in-process; its [`ops.rs`](https://github.com/gitbutlerapp/gitbutler/blob/fefe5663b45e2d72a0a8ad1faabb9668f9444578/crates/but-gpui/src/backend/ops.rs) refuses partial selection of converted or large/binary files. Inspected GPUI paths have no separate LFS control. The repo still contains the Tauri desktop app. | Whole-file selection and an ordinary binary preview are sufficient for this slice; partial LFS hunks remain blocked. GitButler's hook behavior cannot be copied under Kagi's hook policy. |

## Architecture review

1. **SEARCH:** `special_repo::FilterCheck` owns attribute guards;
   `status::working_tree_status` owns status; `staging` owns Stage;
   `ops/<feature>.rs` owns plan/preflight/execute; `cli::run_git` owns Git
   subprocess hardening. These paths already handle the corresponding non-LFS
   operations.
2. **OWNER:** Extend `kagi-git::Backend` and these existing owners. No Git
   repository I/O or LFS process belongs in UI or `kagi-domain`.
3. **DECISION:** EXTEND. A focused LFS helper under the Git backend may hold
   pointer validation and the trusted `git-lfs` invocation; it is not a second
   operation controller or state store.
4. **REFACTOR:** Share one attribute classification across status, Stage, and
   worktree safety checks. Keep unknown named filters rejected.
5. **ANTI-PATTERNS:** Reject UI-side Git calls, parallel mutation pipelines,
   silent backend switching, repo-provided executable filters/hooks, and UI
   fields that duplicate repository state.

## Decision

### Supported format and execution boundary

- Recognize LFS by effective path attributes, including nested `.gitattributes`
  and `.git/info/attributes`; validate pointer format separately. `diff=lfs` or
  `merge=lfs` without `filter=lfs` remains suspicious and blocked for writes.
- Support the standard v1 SHA-256/size pointer and ordinary `filter=lfs`. Named
  filters other than `lfs`, LFS extensions, custom transfer adapters, and
  nondefault LFS storage require an explicit later decision. Never interpret a
  filename extension or a pointer-looking blob alone as permission to run LFS.
- Use the installed official `git-lfs` executable for conversion/transfer,
  invoked only after the user's operation is admitted. Preserve `run_git`'s
  repository-config hardening and hook suppression. Do not run `git lfs install`,
  overwrite hooks, or change global Git config automatically. Missing Git LFS
  produces an actionable refusal for an LFS operation.
- No external process, local LFS object creation, index write, or worktree write
  occurs in a read, plan, or preflight. An unreferenced local LFS object produced
  during an approved Stage before index publication is safe to reclaim later.

### Initial operation scope: View, Stage, Commit, Push

- Status compares a worktree LFS file against the index pointer by SHA-256 and
  size when libgit2 reports a possible worktree change. A pointer left in the
  worktree by explicit skip-smudge is a clean placeholder. The first release
  does not add a dedicated missing-object state or LFS diff renderer. Hashing
  streams the file. The backend read is synchronous; existing UI call sites that
  invoke status directly can pause on very large CAD files. The stacked UI PR
  should move its LFS status reads to background preparation where feasible.
  This latency is a known initial limitation. The read never changes staged
  content. Stat-cache repair remains opt-in on the UI refresh path.
- Whole-file Stage uses the existing Stage admission. For every present LFS
  target, stream its regular-file content through the trusted clean conversion,
  validate the returned pointer against the input bytes, then update the in-memory
  index. A batch validates all paths first and publishes the index once. A
  missing executable, bad pointer, changed attributes, or failed conversion
  leaves staged `(path, OID, mode)` unchanged. Deletion and Unstage keep their
  existing index-only semantics. LFS hunk Stage stays unavailable.
- Before every Git push that can publish commits containing LFS pointers,
  upload the approved commit's *new* LFS objects to the approved remote. Do not rely on
  any pre-push hook. A failed or unknown LFS upload stops before Git ref
  publication and is recorded with the existing outcome/oplog rules. A Git
  push failure after successful LFS upload can leave unreferenced remote LFS
  objects; no remote ref has been moved by the upload itself. Tag and lease
  push paths must use the same guard; delete-only pushes need no object upload.
  Use Git LFS's ordinary remote-tracking range: `--all` fails for legitimate
  skip-smudge clones missing older local objects that the remote already has.
  Repairing an already-broken remote is a separate, explicit operation.
- Existing lockable paths require correct lock verification before a push.
  The initial fixture does not use `lockable`; until verification is implemented,
  block an affected push rather than silently bypass it. Lock/Unlock UI is not
  required for this slice.

### Worktree writes and presentation

- The first release does not implement LFS materialization for checkout, pull,
  merge, revert, snapshot restore, or other worktree-changing operations. The
  generic plan and execution boundary refuses these operations when a current
  indexed path or a locally selected target tree uses `filter=lfs`. Pull and
  Switch recheck the fetched target before any worktree write. Existing per-path
  guards continue to refuse tracked Discard and hunk Stage. Generated fixtures
  cover incoming-only Checkout and Pull. The first release still does not
  claim that every less common worktree writer and external Git process has an
  LFS-safe materialization path.
- Later worktree support must prepare and verify every needed LFS object before
  changing the worktree, preserve approved dirty content, materialize real
  bytes (or an explicitly requested skip-smudge placeholder), and verify final
  index/worktree state.
- Reuse the existing file list, diff, operation card, footer, and oplog.
  Binary LFS files are selectable as whole files. Show specific setup and
  transfer failures through those surfaces in EN and JA; no new top-level LFS
  screen or repository-wide mode is needed. Progress detail is a later UI slice.

## Acceptance matrix

| Boundary | Evidence required |
|---|---|
| Backend read | A clean real-content LFS file stays clean after mtime-only change; a changed file is modified; a skip-smudge pointer stays distinguishable; all reads preserve index `(path, OID, mode)` and make no LFS subprocess call. |
| Backend Stage | Single/batch Stage stores real content locally and only the matching pointer in Git; mixed LFS/plain batches are all-or-none for index publication; unavailable tool and late attribute drift leave staged content unchanged. |
| Backend Push | A local bare remote receives LFS bytes before the Git ref; an upload failure leaves the remote ref unmoved; a timeout is Unknown; custom hooks are not invoked. |
| Backend worktree | Unsupported writers refuse current-index and selected-target LFS paths before mutation; Pull and Switch recheck after fetch; tracked Discard and hunk Stage stay blocked. Incoming-only Checkout and Pull are fixture-tested. |
| UI | A stacked UI PR checks existing file selection, Stage/Push result previews and errors in Tier A and the real EN/JA window in Tier B. The backend suite runs without GPUI. |

## Rollout

The backend PR verifies generated fixtures and the consumer's nested attribute
behavior first. A UI PR stacks on it and verifies Tier A and Tier B separately.
Do not call this subset "Git LFS supported" without naming unavailable worktree
operations.
