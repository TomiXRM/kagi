# ADR-0210 — Applying a PR review suggestion from the conversation (head-blob gate, ref-backed backup)

Status: Accepted
Issue: #351 (parent #359)
Date: 2026-10-01
Amends: ADR-0172

## Context

ADR-0172 landed the backend for applying a ```suggestion block to the working
tree, but no way to reach it from the app, and three choices that do not hold
once a user can click it:

1. **Which lines?** A review comment's `line` / `start_line` are line numbers
   in the PR head's version of the file. ADR-0172 only compared the anchored
   range with what it read when the suggestion was opened. A working-tree file
   with lines added above the range still passes that check if the range text
   happens to repeat — and then the suggestion lands on other lines than the
   ones reviewed.
2. **Recovery.** The pre-apply content was a bare `repo.blob`: nothing kept it
   alive, so a `git gc` could prune the only copy of hand edits the apply
   overwrote. #523 / ADR-0179 introduced `refs/kagi/backups/` precisely for
   working-tree rewrites (discard, worktree remove).
3. **Line endings.** `Suggestion::apply_to` rebuilt the whole file with `\n`,
   turning a CRLF file into an LF file — a whole-file diff for a one-line
   suggestion.

## Decision

- **Head-blob gate.** `Operation::ApplySuggestion` carries the PR head commit.
  The plan compares the working-tree file's blob hash (`Oid::hash_file`, no
  write) with the head's blob at that path (`blob_ids_at`, the #832 read). A
  mismatch is the blocker `SuggestionNotPrHead`; a head commit missing from the
  object store is `SuggestionHeadUnavailable`. Only when the file *is* the head
  version does the existing range check (`SuggestionRangeGone` /
  `SuggestionStale`) run. Because `run_recorded` re-plans at preflight (#502),
  the same gate refuses a file edited between confirm and execute; the
  execute-time range guard stays as the last line.
- **Ref-backed backup.** Execute writes the pre-apply bytes through
  `ops::backup::write_blob` (`refs/kagi/backups/<op>/0`). `SuggestionOutcome`
  carries the `reference`; the oplog entry lists it in `backup_refs` and in the
  typed recovery handle. The plan's recovery reads
  `git cat-file blob <backup-ref>`.
- **Destructive, no equivalent command.** The plan is `destructive: true` (the
  auto-snapshot runs first) with `equivalent_command: None` — git has no
  command for "splice a review suggestion".
- **One suggestion per operation.** No batch apply; each click plans one.
- **Line endings are the file's.** `apply_to` splices at line granularity and
  keeps every line outside the range byte for byte; replacement lines take the
  replaced range's line ending, and a missing final newline stays missing.
- **UI.** A review conversation entry (ADR-0136) whose suggestion resolves
  shows "Apply suggestion…" next to its suggestion chip. It plans against the
  open PR tab's head and opens the shared plan card
  (`ActiveModal::ApplySuggestion`); Confirm / Enter runs it through
  `finish_run`, Cancel / Esc closes it. Nothing is staged — the change waits
  in the Commit Panel. When the thread overlay on the diff lands (#351 second
  pillar), the same button moves there.
- `apply-suggestion` joins reconcile's `writes_only_locally` family, so an
  interrupted apply is acknowledgeable like `discard`.

## Consequences

- A suggestion can only be applied on a checkout where the file is the PR
  head's version; a branch with local edits to that file is refused with the
  reason, not silently mis-applied.
- Hunk-level staging still does not exist (#357). The applied change can be
  reviewed in the Commit Panel's diff and staged per file.
- The `review it with hunk staging` wording in the working-tree-only warning
  now points at the Commit Panel.
