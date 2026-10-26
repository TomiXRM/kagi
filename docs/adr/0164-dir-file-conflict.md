# ADR-0164: Directory/file conflict classification and resolution

- Status: Accepted
- Issue: #320 (follow-up to #302 / PR #318)
- Date: 2026-09-04

## Context

`collect_conflict_files` (`crates/kagi-git/src/conflicts.rs`) classifies content,
rename/delete, modify/delete, add/add, submodule, symlink and binary conflicts
(#318). It had no arm for a **directory/file** conflict — a path that one side
committed as a file and the other as a directory. Such a path fell through to
`ModifyDelete`, offering only keep-or-delete of the file and no way to express
"keep the directory instead". #318 explicitly deferred this because it needs a
UI affordance, not just a git-layer change.

### How a D/F conflict actually appears

Kagi merges with libgit2 (`repo.merge`), not the git CLI. Verified against a real
fixture (both merge directions), libgit2 records a D/F conflict as:

- a **one-sided** unmerged entry at `path` — the file that lost the race
  (stage 2 if ours, stage 3 if theirs, ancestor absent), and
- clean **stage-0** entries under `path/…` — the directory side.

The stage-0 children never appear in `index.conflicts()`, so the collision is
invisible to the conflict iterator alone. (The git CLI differs: it renames the
loser to `path~BRANCH`, which is why a CLI-driven test fixture cannot reproduce
the state kagi sees — the tests drive `repo.merge` directly.)

## Decision

1. **Classification.** Add `ConflictKind::DirFile`. In `classify_kind`, a
   one-sided conflict entry whose name is also occupied by a `path/…` index entry
   is a `DirFile` (checked before the stage-pattern match that would otherwise
   read it as modify/delete). `is_raw()` stays `false`: it is neither a text nor a
   single-blob raw conflict.

2. **Pure FSM.** `kagi_domain::resolution::DirFileChoice { KeepDirectory,
   KeepFile }` — two terminal states, no hunk model. Pure data (invariant #2).

3. **Resolution triple.** `crates/kagi-git/src/ops/dir_file_conflict.rs` holds
   `plan_ / preflight_ / execute_dir_file_resolution` (invariant #4):
   - *KeepDirectory*: `conflict_remove(path)` — the stage-0 `path/…` children
     remain, so the tree keeps the directory.
   - *KeepFile*: `remove_dir(path, 0)` + `conflict_remove(path)` + stage the file
     blob (OID + mode) at stage 0, so the tree keeps the file.
   After the index surgery, the losing namespace is removed from the worktree
   and the kept side is rebuilt. A kept symlink file side is written as a
   symlink and never dereferenced (#298). A recovery snapshot is taken before
   mutation; `preflight_` re-plans and compares before executing (TOCTOU guard).

4. **Oplog.** A D/F resolution is a conflict-lane op (like `conflict-save`: it
   stages into the index and is re-detected away). C1 routes both actions through
   one Backend recorded boundary: `run_recorded_conflict` is the **sole** durable
   receipt writer and returns its recording result with typed progress evidence.
   The UI only presents that receipt. The legacy
   `execute_dir_file_resolution` facade retains its compatibility record until
   its remaining non-app callers migrate; the two entry points share one
   preflight/mutation primitive and are never composed.

5. **UI.** `KagiApp::resolve_dir_file` submits a revision-bound app request and
   applies the Backend's fresh observation (mirroring `conflict_editor_save`).
   For a `DirFile` file the
   conflict center renders two buttons — Keep directory / Keep file — plus a hint,
   instead of the text/buffer choose row. EN + JA `Msg` added (invariant, i18n).

## Consequences

- The Continue gate already blocks on any file lacking a resolution; a `DirFile`
  file has none until the user picks a side, which stages it and re-detects it
  away — so it composes with the existing gate for free.
- No new destructive commands (invariant #3); the index choice is followed by
  the existing recovery-backed worktree namespace reconciliation.
- The focused GUI scenarios exercise the Save and D/F adapters one at a time;
  the window-free application tests own the revision, admission, progress and
  exactly-once receipt matrix.

## Git-started namespace and deletion amendment (#1734 / #1735, 2026-10-25)

The CLI/libgit2 distinction above describes the original implementation, not a
reason to refuse repositories whose merge was started outside Kagi. Extend the
same detector and namespace operation to Git's parked file: for example,
`dir-vs-file~HEAD` at an unmerged stage plus `dir-vs-file/child.txt` at an
unmerged or clean stage. The original path is the logical conflict target.
Index and source-tree identity must positively attribute the parked file to
that namespace; a filename suffix alone is never ownership evidence.

The immutable D/F plan freezes the actual file-side path, child stage/OID/mode
identities and exact worktree removals. Preflight compares those facts before
the existing recovery snapshot or any mutation. Keep directory removes the
parked file, resolves the directory-side stages and retains/stages the children.
Keep file removes the approved directory entries, restores the chosen file at
the **original** path and stages that original path. Both remove the parked
artifact. Verification checks the index and worktree, not merely that a stage-0
entry exists. Unrelated files and lookalike suffix names are outside the plan.
Plans refuse unsupported gitlink/special modes and nested repository metadata
before mutation: the ordinary worktree snapshot cannot preserve a nested object
store, and recovery capture may fail even for keep-directory. Both choices refuse
at planning rather than promising unavailable recovery. Preflight repeats after
recovery capture and before the first write.

The repeated-preflight boundary acquires the real Git `index.lock`, distinct
from Kagi's repository writer lease. Freeze every unaffected index entry's raw
path, stage, OID and mode under that lock, preserve the complete private index,
publish it atomically and retain the lock through verification. Exact unaffected
entry equality is mandatory; a post-write comparison alone cannot detect an
external update already overwritten by a stale in-memory index. Compliant Git
writers cannot enter that read/write window. A noncooperating writer is not
covered by Git's lock protocol and must not be described as excluded by Kagi's
repository lease.
Lock-acquisition errors retain the actual `index.lock` path so presentation can
classify contention as IndexLocked. Before serializing the ownerless private
index, smudge stage-0 non-gitlink entries whose mtime is at least the locked real
index's timestamp by setting their cached size to zero. Publishing a newer index
must not make a same-timestamp, same-size working-file edit falsely clean.

Directory checkout verification uses filter-aware index-to-worktree comparison,
not raw file hashes: `eol=crlf`, `ident` and `core.autocrlf` may legitimately
change checked-out bytes. Nested directories are removed child-before-parent;
symlinks inside the namespace are unlinked, never traversed, and symlinked
ancestors are refused.

Delete and D/F receipts use a real post-execution repository summary. A measured
failed mutation is Partial; once mutation starts, an unreadable after-state is
Unknown, not Success or an invented successful deletion string. Deterministic
fault points cover both completed and interrupted mutations with unreadable
after-state, and an interrupted working-tree mutation with a readable partial state.

The existing conflict-family values also express deletion explicitly:
`Resolution::Delete` is a family choice and `ConflictDraft::Delete` is the frozen
request. DD has only Remove from Index… with a separate deleted-on-both-branches hint; either
modify/delete orientation has Use deleted side and Use modified side. Neither
an absent-side blob nor a zero-byte Result represents deletion. The delete
operation uses its own plan/preflight/execute/verify triple under the same
recorded conflict boundary, preserves recovery and removes only approved
conflict entries/artifacts. The two renamed destinations of a DD source are
not deletion targets.

File-local compact actions open the standard plan sheet for deletion and both
D/F choices. It names the action and target in a sentence-style title, the result
verb on its confirm button, and exact removed/kept/moved paths in EN/JA, with no
command syntax. Index-only removal says Remove; working-file removal instead
says Delete in both title and button. Directory entries end in `/`; parked-file
restoration is one move row. Freeze the exact next-step label when the sheet
opens: Review commit for the last merge conflict, Review to continue otherwise.
The trigger is Secondary. Confirmation is Primary for index-only removal and
Destructive only when working files are removed; only that latter case shows
the working-file warning. Cancel/Escape are safe and root Return confirms neither
role implicitly. Confirmation retains the frozen
owner and plan token through the existing draft-flush and writer-admission
boundaries; a changed repository requires a new plan, not renewed approval.
The sidebar remains navigation and session actions retain #1764's placement.

Design evidence:
[architecture and figure](https://github.com/TomiXRM/kagi/issues/1734#issuecomment-6394541317),
[independent HIG design review](https://github.com/TomiXRM/kagi/issues/1734#issuecomment-6394580602).
The formal ledger adds scoped deletion/parked-namespace HOLD checks and red
variants; the original baseline had no corresponding REFUTE rows to relabel.
