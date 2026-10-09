# Agent workflow: PRs and multi-agent work

Linked from `AGENTS.md`. These rules come from what went wrong while several
agents worked on Kagi in parallel (2026-10-01); where an incident shaped a rule,
the rule names it.

## Working on a PR: push, review, merge

- **Never rewrite a pushed branch.** No force push, no rebase of a pushed branch:
  take the PR's current base (`origin/main`, or the parent branch of a stacked PR)
  in with a merge. After every push, confirm the remote head equals the local one:
  `test "$(git ls-remote --exit-code --heads origin refs/heads/<branch> | cut -f1)" = "$(git rev-parse HEAD)"`.
  A non-fast-forward push is rejected, and "pushed" without that check has been
  wrong; a plain `ls-remote origin <branch>` exits 0 with no output for a typo and
  can match a tag of the same name.
- **Conflicts**: `CHANGELOG.md`, ADRs and `docs/decisions.md` keep both sides.
  **Code conflicts are resolved by reading each hunk**, never by a mechanical
  "keep both": two PRs adding fields to one struct concatenate into a broken brace
  structure (#900). If `rerere` learned a bad resolution, redo the merge with
  `git -c rerere.enabled=false merge`. Git can also merge cleanly and still drop an
  attribute line (`#[cfg(target_os = "macos")]` before a `mod` in
  `tests/gui_e2e_runner.rs`, #901) — check the runner after a merge.
- **Gate before push**: build, `cargo test --workspace`, the touched Tier A
  scenarios and `uv run --project ci check-all` pass first. Chain them so a failure
  stops the push (`set -e`, or capture the output and test it) — never with `;`.
- **Codex review**: read every Codex line comment before a PR is merged, and reply
  to each one (in Japanese, per "Code Review Rules" in `AGENTS.md`). Fix every P0 and P1, and any P2
  that can lead to a wrong write, data loss or a misleading action. A theoretical case (a race
  with another process replacing the repository, a millisecond window) is closed
  with a reply that gives the reason, and the limit is written into the ADR. Aim
  for one review round per PR; Codex keeps finding the next corner otherwise.
- **User-authorized substitute review**: when GitHub Codex cannot run, an
  explicitly authorized, fresh independent Codex/Sol reviewer may review the
  exact head through herdr. Publish its model, agent identity, reviewed SHA,
  read set, findings and Japanese responses on the PR. State that the GitHub
  bot did not run; static review is not runtime verification. The reviewed-head,
  green-CI and unanswered-comment merge gates still apply.
- **Merge** only the head commit Codex reviewed, with CI green and no unanswered
  Codex comment: `gh pr merge N --merge --match-head-commit <sha>`, then confirm the
  PR reads `MERGED`.
- **Stacked PRs** set their base to the parent branch and say "change the base to
  main after #N merges" at the top of the body.
- `Closes #N` only when the PR meets every acceptance criterion of #N; otherwise
  `Refs #N` and list what remains. Split a leftover into its own issue.

## Multi-agent work over herdr

When a PM session drives implementation agents in herdr panes:

- **Mail is the only task transport.** PM assignments, amendments, decisions and
  agent replies use herdr mail, not `agent prompt`, `pane send-text` or simulated
  terminal Enter. Read the installed mail command help and verify support in the
  selected server before dispatch. If unavailable, report the missing capability
  to the PM and pause dispatch; do not silently fall back or restart/upgrade a
  shared server.
- **Every assignment names its contract.** Include a stable task name, sender and
  reply destination, session/workspace, owned worktree/branch, Issue/PR and exact
  head when applicable, scope, acceptance criteria and prohibited actions.
  Resolve addresses from live IDs; do not infer them from pane order.
- **Replies must also use mail.** A recipient sends `[ack]` before starting, then
  replies in the same task/thread with `[done]`, `[status]`, `[ask]` or `[info]`.
  Include the task name and exact head in both receipt and final report.
  `[done]` names the actual changes or reviewed files, findings/responses,
  observed checks and evidence links; an implementation report includes remote
  head verification and UI Tier B links where required. Explicitly distinguish
  unrun checks and static review from runtime proof. `[ask]` names the blocker
  or decision and pauses the affected action until the PM replies by mail.
- **Delivery is not execution.** Retain the mail ID/thread and require the
  recipient's `[ack]`; queued/sent does not prove work started. Inspect the
  thread after ambiguous delivery before retrying, avoiding duplicate work.
- **One agent, one worktree, one branch.** Never edit another agent's worktree or
  push to a branch another agent owns without saying so first. A PM that merges
  `main` into an agent's PR branch tells that agent before it pushes again.
- **Verify the premise before assigning.** Before writing "reuse the existing X",
  search for X — issue bodies go stale. An agent that finds the premise wrong says
  so instead of building around it.
- **Messages are scoped to the workspace.** A prompt about another repository or an
  issue number that does not match this repository is a misroute: report it, do
  not act on it.
- Watch usage limits in `herdr agent list` (`limit`); an agent near its limit gets
  small, finishable tasks.
