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
   No working-tree write happens (a kept symlink file side is never dereferenced,
   #298). `preflight_` re-plans and compares before executing (TOCTOU guard).

4. **Oplog.** A D/F resolution is a conflict-lane op (like `conflict-save`: it
   stages into the index and is re-detected away), not a `Backend::run`
   `Operation`. So `execute_dir_file_resolution` is the **sole** oplog writer for
   the op (`conflict-dir-file:<choice>`); the UI records with `record_op`
   (non-persist) to avoid a double record. No write path is left unlogged.

5. **UI.** `Backend::execute_dir_file_resolution` + `KagiApp::resolve_dir_file`
   (re-detect after, mirroring `conflict_editor_save`). For a `DirFile` file the
   conflict center renders two buttons — Keep directory / Keep file — plus a hint,
   instead of the text/buffer choose row. EN + JA `Msg` added (invariant, i18n).

## Consequences

- The Continue gate already blocks on any file lacking a resolution; a `DirFile`
  file has none until the user picks a side, which stages it and re-detects it
  away — so it composes with the existing gate for free.
- No new destructive commands (invariant #3); resolution is index-only.
- GUI behavior (the two buttons and re-detect) is not exercisable by subagents and
  needs human verification; build + tests pass.
