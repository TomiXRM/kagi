# ADR-0184: Armed unmerged branch deletion with retained tips

- Status: Accepted for implementation (#584)
- Related: ADR-0046, ADR-0178, ADR-0179; existing delete branch (ADR-0014)

Unmerged local branches become warnings, not blockers. The warning counts commits
that would become unreachable from other refs/HEAD without the mandatory recovery
ref. Symbolic refs whose chain depends on the deleted ref are excluded from
independent roots; a direct ref at the same tip remains a root. Detached-at-tip
and trust refusals stay. Any checked-out branch in the main or a linked worktree
is refused, including clean worktrees; delete-branch never removes a worktree.
Squash-proven and merged branches keep one confirmation; unmerged deletion uses
`DeleteBranchModal.confirm_armed` at the common Enter/button `start_delete_branch`
entry. Opening/reopening or failed execution resets the arm. CLI/MCP keep their
existing explicit approval contracts; a GUI click count is not a Backend token.

The frozen recovery tip is a full OID. Execution opens main plus all registered
linked worktrees, locks each HEAD and the target branch ref, and rechecks occupancy,
registration inventory, planned HEAD and full tip before side effects. Unreadable
registrations and lock failures refuse deletion. Existing HEAD locks remain held
through deletion; an inventory change requires re-planning. It pins the tip through the existing
`refs/kagi/backups/<attempt-id>/...` helper before removal. No optional snapshot
setting disables this root. The single Backend receipt carries the full tip and
restore command plus structured `backup_refs`, including roots made before an
error. Restoration uses the existing create-branch/`git branch` route with that
exact backup ref, preserving the entire commit ancestry after GC. No new restore
executor or UI Git access is introduced. The branch reflog is deleted under the
same branch ref lock before committing removal: cleanup failure leaves the branch
and recovery root intact, and cleanup never races a recreated same-name ref.

Commit roots share ADR-0179's retention: indefinite by default, retired only with
the final referencing oplog entry. Blob readers stay blob-only; retention accepts
both blobs and commits. The sidecar lock/sequence contract is unchanged.
The async plan freezes `Attachment` (SessionId, WorktreeId, path and visit) plus
the switch generation. Completion first releases its plan busy latch, even on stale delivery or task
failure. Stale delivery then returns before modal/footer updates;
confirm must match that attachment, and execution reopens its frozen path and
compares WorktreeId before Backend admission. A tab departure/revisit cannot
revive the old approval, even if branch names and tips coincide.

Existing klog lines and ordering remain; only a new armed line is added. Successful
execution logs completion even when durable recording fails. The owner notice,
Partial presentation and suppression of retry remain, without a second append. Recovery
and warning text and the armed button label have EN/JA representations.
