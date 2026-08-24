# ADR-0140: Tags can be published from kagi

- Status: Accepted
- Date: 2026-08-24

## Context

kagi could create a tag (from a commit's context menu) and then only look at
it. `refs/tags/` never left the machine, so publishing a release tag meant
switching to a terminal — in a tool whose whole point is that you should not
have to. Tags were also the only ref in the sidebar with no context menu at
all.

## Decision

Add the `plan_push_tag` / `execute_push_tag` pair to
`crates/kagi-git/src/ops/tag.rs`, reachable from a new tag context menu
(right-click a tag in the sidebar).

**Guarded, not Destructive.** The push adds a ref on the remote; it never
moves or removes one. So it gets a single confirm rather than the armed
two-stage that delete-remote-branch and force-with-lease use. What it *does*
get is a warning saying out loud that this leaves the machine — every other
tag action in kagi is purely local, and that asymmetry is worth stating.

**Never forced.** `execute_push_tag` runs `git push <remote> refs/tags/<name>`
with no force flag, ever. If the tag already exists on the remote pointing at
a different commit, the remote refuses the push. That refusal *is* the safety
property: kagi cannot know what a moved tag would break for everyone who
already fetched it, so the right answer is to let the remote say no and
surface its message verbatim. `a_moved_tag_is_refused_by_the_remote_not_forced`
in `tests/push_tag_test.rs` pins it, asserting the published tag did not move.

The refspec is fully qualified (`refs/tags/<name>`) so a branch of the same
name can never be pushed by accident.

**Remote choice** reuses `push`'s `choose_push_remote` — `origin` if it
exists, else the first configured remote — so the tag menu and the push button
can never disagree about where things go. The menu label names the remote
(`Push tag to origin`); with no remote configured the item is shown
**disabled with the reason** rather than hidden, so the action stays
discoverable.

## Consequences

- Undo is `git push <remote> --delete <tag>`, and the recovery text says
  plainly that this only helps until someone else fetches it. A published tag
  that others have pulled cannot be recalled, and kagi should not pretend
  otherwise.
- Annotated tags are pushed the same way as lightweight ones (the refspec does
  not care), but kagi still only *creates* lightweight tags — unchanged here.
- No "push all tags": `--tags` publishes every local tag at once, including
  scratch ones, which is exactly the kind of bulk remote side effect this
  product avoids. One tag, named, per confirmation.
- There is still no delete-tag (local or remote). Deleting a published tag is
  the genuinely dangerous direction, and it can wait for its own ADR.
