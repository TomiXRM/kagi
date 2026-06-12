# QA Audit — Operation × State Combinatorial Matrix

**Date:** 2026-06-13
**Trigger (user report):** 「commit を stage した状態で stash しようとしたらフリーズした。そういう機能の組み合わせの未検証は色々ありそうなので調査して。」
**Method:** Headless `KAGI_*` env drivers + `KAGI_AUTO_CONFIRM=1`, each case run in a fresh `TempDir` fixture, background-launch + sleep + kill freeze detection (macOS, no `timeout`). Op-phase stderr (`[kagi] plan: …` / `executed:` / `verified:`) parsed; `git fsck` run after every executing op.

**Headline:** No data loss, no repo corruption, no panics were found anywhere. The guard-rail (plan → blockers/warnings → confirm → execute → oplog) architecture is sound for *safety*. The user's freeze is **real and systemic**: every operation executes libgit2 **synchronously on the GUI thread**, with no `cx.background_spawn`. For network ops (pull/push) this is an unbounded hang; for stash/checkout/commit it is proportional to working-tree size.

---

## 1. Matrix (state × operation)

Legend: `OK` clean expected result · `WARN` correct blocker/warning surfaced · `BLOCK` operation correctly refused · `SLOW` op-phase ran on UI thread ≥ notable time (async candidate) · `HUNG` op-phase did not complete within timeout · `BUG` plan/execute contradiction · `—` not applicable / skipped by design (e.g. unborn HEAD).

| State \ Op            | STASH_PUSH | STASH_POP | STASH_APPLY | UNDO  | AMEND (3) | COMMIT | CHECKOUT_COMMIT | REVERT | CREATE_BR | DELETE_BR | PULL  | PUSH  |
|-----------------------|:----------:|:---------:|:-----------:|:-----:|:---------:|:------:|:---------------:|:------:|:---------:|:---------:|:-----:|:-----:|
| clean                 | BLOCK      | BLOCK     | BLOCK       | OK    | OK        | BLOCK  | OK              | OK     | OK        | —         | BLOCK | BLOCK |
| staged only           | OK         | —         | —           | OK    | OK        | OK     | OK              | OK     | OK        | —         | —     | —     |
| unstaged only         | OK         | —         | —           | OK    | OK        | BLOCK  | OK              | —      | —         | —         | —     | —     |
| staged + unstaged     | OK         | —         | —           | OK    | OK        | OK     | **BUG-2**       | —      | —         | —         | —     | —     |
| untracked (×400)      | OK (warn)  | —         | —           | OK    | —         | —      | OK              | —      | —         | —         | —     | —     |
| untracked huge (150MB)| **SLOW**   | —         | —           | —     | —         | —      | —               | —      | —         | —         | —     | —     |
| conflict (MERGING)    | BLOCK      | BLOCK     | BLOCK       | BLOCK | BLOCK     | BLOCK  | —               | BLOCK  | —         | BLOCK     | —     | —     |
| conflict_marker only  | OK         | —         | —           | —     | —         | —      | —               | —      | —         | —         | —     | —     |
| detached HEAD         | BLOCK*     | —         | —           | BLOCK | BLOCK     | —      | OK              | BLOCK  | OK        | —         | —     | —     |
| unborn (init)         | BLOCK      | —         | —           | BLOCK | BLOCK     | —      | —               | —      | — (skip)  | —         | —     | —     |
| merge commit @ HEAD   | BLOCK*     | —         | —           | BLOCK | BLOCK     | —      | —               | BLOCK  | —         | OK        | —     | —     |
| multi-stash (×3)      | —          | OK        | OK          | —     | —         | —      | —               | —      | —         | —         | —     | —     |
| upstream unreachable   | —          | —         | —           | —     | —         | —      | —               | —      | —         | —         | HUNG  | HUNG  |

`BLOCK*` = blocked because that fixture happens to be clean (clean merge / detached at clean commit), not because the op rejects the state itself — i.e. "nothing to stash".

**BUG cells: 1** (BUG-2, checkout-commit on staged+unstaged overlapping). All other off-nominal cells are *correct* blockers.

---

## 2. Findings (severity order)

### BUG-1 — `Critical (UX)` — All operations execute on the GUI thread; network ops hang indefinitely
**This is the user's reported freeze, generalized.**

- **Evidence:** Every one of the 14 `confirm_*` methods in `src/ui/mod.rs` calls `execute_*` (and `preflight_check*`) **inline on `&mut self`** with **zero** `cx.background_spawn` / `cx.background_executor` usage. Verified by span scan of: `confirm_create_branch`, `confirm_create_worktree`, `confirm_stash_push`, `confirm_stash_apply`, `confirm_cherry_pick`, `confirm_revert`, `confirm_checkout`, `confirm_pull`, `confirm_push`, `confirm_undo`, `confirm_amend`, `confirm_pop`, `confirm_delete_branch`, `confirm_commit` → spawn hits = 0 for all. (Contrast: avatar fetch and clone DO use `background_spawn`.)
- **`confirm_stash_push`** at `src/ui/mod.rs:2556` calls `execute_stash_push(&mut repo, …, true)` directly at line 2621.
- **Repro A (SLOW, the user's class):** stash-push `-u` with 3×50 MB untracked binaries → `execute` took **3.0 s** on the main thread. libgit2 must copy all untracked content into the stash commit. A larger or slower tree exceeds 5 s and *looks like a hang*; on the UI thread the window is unresponsive for the whole duration.
- **Repro B (HUNG):** push to an unreachable remote (`https://10.255.255.1/…`) with a clean plan (blockers=0) → `execute_push` **never completed within 15 s** (libgit2 connect timeout is tens of seconds). The entire GUI thread is frozen for the full timeout. `confirm_pull` (`:3906`) and `confirm_push` (`:4088`) are both affected.
- **Recommended fix (NOT applied — too large for a QA pass):** move every `execute_*` body into `cx.background_spawn`, return a `Task`, and apply the result back via `cx.spawn(async move |this, acx| …)` (same pattern already used for avatar/clone). Network ops (pull/push/fetch) are the priority; stash/checkout/commit/amend next.

### BUG-2 — `Minor (plan quality)` — checkout-commit plan says "no blockers" but execute can fail on a dirty overlapping file
- **Evidence:** State = staged+unstaged ("mixed"). `plan_checkout_commit` (`src/git/ops.rs:409`) only pushes a **warning** for a dirty tree (`"Working tree is dirty (…). Safe checkout may fail; stash or commit first."`) — never a blocker. Execution uses `CheckoutBuilder::safe()`.
- **Observed divergence:**
  - staged-only, non-overlapping → `execute` **succeeds**, detaches HEAD, staged edit **preserved** (verified on disk + fsck clean).
  - staged+unstaged overlapping the target diff → `execute` **fails**: `checkout_tree failed: 1 conflict prevents checkout`. Plan promised blockers=0.
- **Impact:** No data loss (safe-mode refuses, local edit stays on disk — pinned by test `checkout_commit_overlapping_dirty_fails_without_data_loss`). Pure plan-accuracy/UX gap: user is shown a green "proceed" plan that then errors in the footer.
- **Recommended fix:** in `plan_checkout_commit`, do an in-memory dry-run (analogous to `predict_stash_pop_conflict`'s `merge_commits`) — if the dirty paths overlap the target tree diff, promote the warning to a blocker. *Not applied:* requires real logic + its own test matrix, beyond a 1-line safe fix.

### Non-issues confirmed safe (audited, behaving correctly)
- **stash-push × staged / mixed / untracked** — succeed, tree clean, stash +1, fsck clean. The user's *logic* path is correct; only the *threading* (BUG-1) bites with large trees.
- **stash-pop × dirty tree** — blocked (dirty-tree policy), stash preserved. **stash-pop × conflict prediction** — `predict_stash_pop_conflict` in-memory merge blocks correctly, stash never dropped (apply-then-drop-on-success, ADR-0009).
- **undo × staged/unstaged** — `execute_undo_commit` is a **ref-only soft reset**; working-tree edits survive (pinned by test). No data loss even though plan shows blockers=0.
- **amend × pushed commit** — blocked (published history). **amend × merge/detached/unborn/conflict** — all blocked. All 3 modes (message/staged/both) work on a normal staged HEAD.
- **revert × merge HEAD / conflict / detached** — blocked. **revert × clean/staged** — produces inverse commit, preview_files populated.
- **commit × all-unstaged** — blocked ("nothing staged"). **commit × clean** — blocked. **commit × conflict** — blocked (2 blockers).
- **delete-branch × missing / current / unmerged** — blocked. **create-branch × unborn** — gracefully skipped ("could not resolve HEAD commit"), no crash.
- **context-menu / compare-WT** on detached / unborn / mixed — pure models, no panic.
- **fsck after every executing op** (stash, pop, undo, amend-both, revert, checkout, create-branch, commit, apply) → **CLEAN** in all 9.

---

## 3. SLOW / HUNG list (async-ification targets)

| Op            | Trigger                                  | Measured            | Class | Why it blocks the UI |
|---------------|------------------------------------------|---------------------|-------|----------------------|
| PUSH          | upstream unreachable / slow remote       | **>15 s, no finish**| HUNG  | network I/O on UI thread (`confirm_push`, `:4088`) |
| PULL          | upstream unreachable / slow remote       | unbounded (same path)| HUNG  | fetch + merge on UI thread (`confirm_pull`, `:3906`) |
| STASH_PUSH    | huge untracked (150 MB) with `-u`        | **3.0 s**           | SLOW  | libgit2 copies untracked blobs into stash on UI thread |
| CHECKOUT_COMMIT | very large tree / many changed files   | scales w/ tree      | SLOW  | `checkout_tree` writes working tree on UI thread |
| COMMIT / AMEND | very large staged tree                  | scales w/ tree      | SLOW  | tree-build + write on UI thread |
| CHERRY_PICK / REVERT | large diff                        | scales w/ diff      | SLOW  | in-memory merge + index write on UI thread |

**Priority order to async-ify:** PULL, PUSH (network, can hang forever) → STASH_PUSH, CHECKOUT_COMMIT, COMMIT (tree-size proportional) → the rest.

---

## 4. Reproduction

Fixtures and drivers (all tempdir, no force):

```bash
# BUG-1 / Repro B (HUNG): push on UI thread to unreachable remote
d=$(mktemp -d); git init -q -b main "$d"; cd "$d"
git config user.name T; git config user.email t@e; git config commit.gpgsign false
echo a>f; git add f; git commit -qm c1
git remote add origin https://10.255.255.1/x.git
git config branch.main.remote origin; git config branch.main.merge refs/heads/main
echo b>>f; git commit -qam c2
( KAGI_PUSH=1 KAGI_AUTO_CONFIRM=1 kagi "$d" & P=$!; sleep 15; kill -0 $P && echo HUNG; kill -9 $P )

# BUG-1 / Repro A (SLOW 3s): stash huge untracked on UI thread
d=$(mktemp -d); git init -q -b main "$d"; cd "$d"
git config user.name T; git config user.email t@e; git config commit.gpgsign false
echo a>README; git add README; git commit -qm c1
for i in 1 2 3; do dd if=/dev/urandom of=big_$i.bin bs=1m count=50 2>/dev/null; done
echo x>>README; git add README
time ( KAGI_STASH_PUSH=1 KAGI_AUTO_CONFIRM=1 kagi "$d" )   # execute ~3.0s before window

# BUG-2: checkout plan blockers=0 but execute fails on overlapping dirty file
#   (driven by KAGI_CHECKOUT_COMMIT=1 KAGI_AUTO_CONFIRM=1 on a staged+unstaged tree)
```

Pinned regression coverage: `tests/qa_audit_test.rs` (10 tests, all `TempDir`, fsck-asserted). **Pull/push HUNG cases are deliberately excluded from the test suite** (they would block CI on a network timeout) — they live only in this doc's repro section.

---

## 5. What was changed in this audit

- **Added** `tests/qa_audit_test.rs` — 10 regression tests pinning the safe findings (stash matrix, undo soft-reset safety, pop dirty-block, checkout dirty plan/execute contract + data-safety, amend-pushed block). No existing test touched.
- **Added** this document.
- **No production code fixed.** BUG-1 (async-ify all ops) and BUG-2 (checkout dirty dry-run blocker) are real-logic changes outside the "self-evident 1-line safe fix" scope and are left as recommendations with reproduced evidence.
