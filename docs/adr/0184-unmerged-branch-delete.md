# ADR-0184: Armed unmerged branch deletion with retained tips

- Status: Accepted for implementation (#584)
- Related: ADR-0046, ADR-0178, ADR-0179; existing delete branch (ADR-0014)

Unmerged local branches become warnings, not blockers. The warning counts commits
that would become unreachable from other refs/HEAD without the mandatory recovery
ref. Current-branch, detached-at-tip, dirty/locked worktree and trust refusals stay.
Squash-proven and merged branches keep one confirmation; unmerged deletion uses
`DeleteBranchModal.confirm_armed` at the common Enter/button `start_delete_branch`
entry. Opening/reopening or failed execution resets the arm. CLI/MCP keep their
existing explicit approval contracts; a GUI click count is not a Backend token.

The frozen recovery tip is a full OID. Execution locks HEAD attachment and the branch ref, then rechecks the
planned HEAD and full tip before side effects, then pins that commit through the existing
`refs/kagi/backups/<attempt-id>/...` helper before removal. No optional snapshot
setting disables this root. The single Backend receipt carries the full tip and
restore command plus structured `backup_refs`, including roots made before an
error. Restoration uses the existing create-branch/`git branch` route with that
exact backup ref, preserving the entire commit ancestry after GC. No new restore
executor or UI Git access is introduced.

Commit roots share ADR-0179's retention: indefinite by default, retired only with
the final referencing oplog entry. Blob readers stay blob-only; retention accepts
both blobs and commits. The sidecar lock/sequence contract is unchanged.
Existing klog lines and ordering remain; only a new armed line is added. Recovery
and warning text and the armed button label have EN/JA representations.
