# Raw executor migration inventory (#566)

The 61 former public functions below no longer admit external Git mutations.
Plans, read APIs and data types remain public. Root and `ops` re-exports preserve
the crate-private visibility of implementations. The former absorb/stash-pop
wrappers are removed; the simple remove wrapper exists only in crate unit tests.

| Implementation | Former public names | Public route |
| --- | --- | --- |
| `conflicts.rs` | `execute_conflict_continue`, `stage_conflict_resolution`, `execute_conflict_save`, `execute_merge_commit`, `execute_conflict_abort`, `execute_stash_conflict_abort`, `execute_conflict_skip` | Dedicated Backend conflict methods; `run(MergeCommit)` |
| `ops/absorb.rs` | `execute_absorb` | Dedicated Backend methods |
| `ops/branch.rs` | `execute_create_branch`, `execute_rename_branch`, `execute_delete_branch` | `Backend::run` |
| `ops/branch_cleanup.rs` | `execute_delete_merged_branches` | Dedicated Backend methods |
| `ops/checkout.rs` | `execute_checkout`, `execute_checkout_commit` | `Backend::run` |
| `ops/cherry_revert.rs` | `execute_cherry_pick`, `execute_revert` | `Backend::run` |
| `ops/dir_file_conflict.rs` | `execute_dir_file_resolution` | `execute_planned_dir_file_resolution` (or immediate UI method) |
| `ops/discard.rs` | `execute_discard` | `Backend::run` |
| `ops/fetch.rs` | `fetch_remote`, `fetch_remote_branch` | Dedicated Backend methods |
| `ops/force_lease.rs` | `execute_force_with_lease_push` | `Backend::run` |
| `ops/history.rs` | `execute_undo_commit`, `execute_amend`, `execute_undo`, `execute_redo` | `run(Amend/UndoCommit)`; `run_history_move` |
| `ops/merge.rs` | `execute_merge_branch`, `execute_merge_into_conflict` | `Backend::run` |
| `ops/merge_into.rs` | `execute_merge_into_branch` | `Backend::run` |
| `ops/pull.rs` | `execute_pull`, `execute_pull_branch_ff` | `Backend::run` |
| `ops/push.rs` | `execute_push`, `execute_push_branch`, `execute_set_upstream` | `Backend::run` |
| `ops/rebase.rs` | `execute_rebase_current_onto` | `Backend::run` |
| `ops/remote_branch.rs` | `execute_delete_remote_branch` | `Backend::run` |
| `ops/reset.rs` | `execute_reset_current_to_head` | `Backend::run` |
| `ops/snapshot.rs` | `create_snapshot`, `prune_snapshots`, `delete_snapshot`, `execute_restore_snapshot` | Backend snapshot methods; `run(RestoreSnapshot)` |
| `ops/stash.rs` | `execute_stash_push`, `execute_stash_apply`, `execute_stash_pop`, `execute_stash_drop` | `Backend::run` |
| `ops/suggestion.rs` | `execute_apply_suggestion` | `Backend::run` |
| `ops/switch.rs` | `execute_checkout_tracking_branch`, `execute_switch_to_latest` | `Backend::run` |
| `ops/tag.rs` | `execute_create_tag`, `execute_push_tag` | `Backend::run` |
| `ops/worktree.rs` | `execute_create_worktree`, `execute_open_worktree_for_branch`, `execute_unlock_worktree` | `run(CreateWorktree/OpenWorktreeForBranch)`; Backend unlock |
| `ops/worktree_lifecycle.rs` | `execute_lock_worktree`, `execute_prune_worktrees`, `execute_repair_worktrees` | Dedicated Backend methods |
| `ops/worktree_remove.rs` | `execute_remove_worktree` | Frozen `RemovePlan` → `run_recorded_remove` |
| `staging.rs` | `stage_file`, `unstage_file`, `execute_commit`, `stage_files`, `unstage_files` | Backend staging methods; `run(Commit)` |

Fixture adapters live in `tests/support/backend_ops.rs` and `tests/support/remove.rs`.
They invoke the public boundary; they do not re-export or conditionally enable raw
executors. No CLI/MCP/example caller needed a bypass exception.

The additionally audited `run_post_create` is now crate-private and unused
`run_pre_remove` is removed. Their trust/headless acceptance tests moved from the
integration target into `ops/worktree_steps_acceptance_tests.rs`; pre-remove's
unapproved-config oracle is a private remove unit test. The dirty-file safe
checkout oracle lives in `ops/merge_executor_tests.rs` because public merge plans
refuse that state earlier.

Explicit config trust grants stay public for the existing UI approval path and
`tests/app_remove_test.rs`; see ADR-0178's grant row. Remaining family-specific
recording/admission work is unchanged, not an exception allowing raw executors.
