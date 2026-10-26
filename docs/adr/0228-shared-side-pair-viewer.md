# ADR-0228: Shared side-pair viewer

- Status: Accepted architecture; steps ②–④ implemented and native functional acceptance passed; independent final HIG review pending
- Date: 2026-10-24
- References: [#1742](https://github.com/TomiXRM/kagi/issues/1742), [initial architecture review](https://github.com/TomiXRM/kagi/issues/1742#issuecomment-6381625496), ADR-0226, ADR-0227; deletion prerequisite #1734

## Context and source-review drift

Diff and raw conflict already have side-pair readers and renderers. Extend those
owners rather than introduce a side-pair module, crate, manager or global cache.
The linked review satisfies the initial issue-comment requirement. Its source
references describe the probe checkout, not permanent line anchors.

The following claims were re-read in the step-① checkout before documentation
changes (line numbers below refer to that pre-consolidation snapshot):

1. `src/ui/main_diff_pane/preparation.rs:167-262` still maps source-faithful
   old/new sides, uses a 10 MiB PDF limit and `usize::MAX` for binary image reads.
   `src/ui/avatar_fetch.rs:252-272` still recognizes PNG/JPEG/WebP/GIF by magic
   and wraps encoded bytes; it does not prove successful pixel decoding.
2. `src/ui/conflict_binary_view.rs:37,61-108,116-135,164-245` still uses an
   8 MiB image limit, extension-first recognition including BMP, thread-local
   `SIDE_CACHE`, and render-time side preparation. These are the consolidation
   inputs, not the intended post-step-① lifetime.
3. `crates/kagi-git/src/backend/file_reads.rs:8-15,59-96` has no frozen-side
   source at this snapshot and bounds application-owned copies, not libgit2
   internal memory. Resolution anchors have drifted: `side_raw_meta` is at
   `crates/kagi-git/src/resolution.rs:698`, `side_bytes` at `708-717`, and
   `side_blob_info` at `722-737`; the uncapped `Option` read still collapses
   object/read failure. The old review's ranges must not be copied as current.
4. `render_diff_list` is now `src/ui/render_helpers.rs:591` (review: 592).
   PDF absence/source/failure remains explicit at `src/ui/diff_view/pdf.rs:18-32`;
   synchronization still defaults to false at `97`, and retirement clears
   subscriptions at `151-159`. Main diff retains by immutable source identity
   (`src/ui/main_diff_pane.rs:76-86`) and guards completion by session, visit,
   request, publication and source (`514-525`).
5. `crates/kagi-domain/src/resolution.rs:14-23` still has only Current,
   Incoming and Both choices; it does not establish a deletion operation.
   The required regression registrations are at
   `tests/gui_e2e_runner.rs:3363-3440,4158-4159`, not the review's old range.
   No #1734 completion, runtime correctness or GUI evidence is inferred from
   this source inspection.
6. The remaining presentation/source boundaries were rechecked during step ①:
   `diff_split.rs:25-85` still projects shared row indices; `diff_state.rs:15-89`
   still omits absent image columns and uses independent Contain; conflict
   `pages.rs:27-66,132-247` and `rows.rs:39-96,427-556` retain aligned A+C
   rows, one source scroll handle and conflict-only adoption. PDF pair render
   starts at `diff_view/pdf.rs:241` (review range starts 238); synchronization
   implementation starts at `201`, with unchanged normalized peer behavior.
7. PR (`pr_mode.rs:1064-1106,1746,1770`), File History (`file_history.rs:63,432`)
   and Editor (`editor_workspace.rs:50-100,451-473,578-582`) still use the
   shared row renderer without main-diff media parity. Editor PDF limits remain
   4096 output side/16M pixels (`pdf.rs:10-11`), 64 MB/two jobs/32 retained
   entries (`pdf_viewer.rs:13-18`). The authoritative raw Save remains
   `conflicts.rs:1424-1466,1580-1605`; no presentation bytes reach stage 0.
   These line shifts do not change the review's owner decision.

## Decision: extend the existing owners

| Responsibility | Single boundary |
|---|---|
| Bytes, metadata, prefix probes | `crates/kagi-git/src/backend/file_reads.rs`: bounded Backend reads, extended for frozen conflict OID/mode; no repository access in render |
| Off-thread preparation | One callable preparation entry in `src/ui/main_diff_pane/preparation.rs`, reached by caller-specific source adapters; conflict does not call `KagiApp::read_main_diff` |
| Pair model and rendering | Extend `src/ui/diff_view.rs`, `src/ui/diff_view/pdf.rs` and existing `render_helpers`/`diff_state` helpers; no new viewer subsystem |
| PDF parsing/raster/viewer | Reuse Editor `PdfSource` and `PdfViewer`; do not duplicate parsing, continuous pages, Fit Width or raster ownership |
| Accepted presentation lifetime | `MainDiffPane` and `ConflictView`, each retaining only its accepted pair |
| Adoption, Change, status, Save and deletion | Conflict owners and `ResolutionBuffer` only; never fields or callbacks with mutation authority on a generic viewer |

Step ① moves raw conflict byte acquisition and media preparation out of render,
replaces the global/thread-local cache with accepted host-owned results, and
shares preparation while retaining explicit legacy policies. Delete the old
cache/build/read path after its consumers migrate, rather than retain aliases.
External comparison materialization must also execute off-thread; it is not a
second inline reader. This does not authorize a rewrite of unrelated operations.

Backend source identity freezes OID and mode, not a mutable index-stage lookup.
A missing stage is Absent; a present but unreadable, oversized or non-blob source
is not Absent. Classify gitlinks by mode without requiring their commit object
in the local object database, and read symlink link bytes rather than targets.
A byte cap is a bound on application copies/decode input, not total process
memory or hostile-document sandboxing.

The obsolete `ResolutionBuffer::{side_bytes, side_blob_info}` and Backend
facades are removed after caller migration. Tests exercise the live
`FileReadSource::ConflictStage` boundary directly, including exact binary
bytes, symlink targets, absence, typed errors and frozen-stage bounds.

## Explicit policy decision and rollout

| Policy | Step ①: preserve behavior | Step ②: shared renderer policy |
|---|---|---|
| Image input cap | Diff remains uncapped (`usize::MAX`); conflict remains 8 MiB | **8 MiB on both surfaces**, checked before application-owned copy/decode input |
| Image recognition | Diff magic-only PNG/JPEG/GIF/WebP; conflict extension-first plus magic, including BMP | **Magic-first union PNG/JPEG/GIF/WebP/BMP** on both surfaces; an extension must not override recognized bytes |
| PDF input cap | Retain existing 10 MiB bounded preparation; do not enable a new conflict PDF surface | **Shared 10 MiB** preparation; conflict presentation is enabled in step ③ |
| Decode status | Encoded image wrapping retains existing behavior | Successful decoding, dimensions and failures must be represented truthfully, not inferred from `Image::from_bytes` |

The shared 8 MiB image cap is a decided behavior change, deliberately applied
in step ② rather than hidden in a behavior-preserving refactor. It bounds copies
and encoded decoder inputs consistently; it does not bound decompressed pixels.
The union retains conflict BMP capability instead of accidentally dropping it
by reusing avatar detection. Avatar/network image policy is outside this change.
Tests that currently assert unchanged diff behavior remain unchanged in step ①;
step ② adds explicit boundary/classification acceptance coverage.

## Proposed presentation contract — not implemented in step ①

The target projection is `SidePair { left, right, headers }` with
`SideContent::{Text, Image, Pdf, Binary, Symlink, Submodule, Absent}`. This is a
proposed contract, not a claim that these Rust types exist or must be introduced
in preparation consolidation. Step ② fits the contract into the existing diff
model and helpers without creating a second mutable media cache.

- Headers carry localized labels, optional role accent, stable side identity,
  accessible names and absent descriptions. The viewer has no selected-side or
  resolved-state authority.
- Text references existing rows/projections, not copied strings. Keep diff's
  unified mode and paired-index single virtualized list; conflict keeps its
  aligned source rows, action rows, shared `ab_scroll` and separate Result editor.
- Image payloads eventually carry reusable handles, encoded byte count,
  dimensions and real decode outcome. The pair computes one scale and origin
  for both sides; two independent `ObjectFit::Contain` calls are insufficient.
- PDF payloads reuse prepared `PdfSource` and retained `DiffPdfViewers`.
  Preserve normalized scroll synchronization and independent zoom; diff starts
  independent, while the conflict PDF feature starts synchronized.
- Binary summaries retain size/OID and explicit unavailable/cap/read/decode
  outcomes. Absence is structural, not a fallback for preparation failure.
- Classify sides independently by frozen mode/content, so mixed media,
  modify/delete and mode changes are not forced through one file-wide kind.
  This renderer/classification cutover is not part of step ①.

Keep existing diff public adapters/probe IDs and source mapping, including
renames, Compare/stash, staged/unstaged and linked worktrees. PR, File History
and Editor diff adapters sharing a dispatcher do not thereby gain media parity.
Keep the existing binary caption owner and generalize headers/body helpers
rather than introducing conflict-only PDF/header/image renderers.

## Lifetime and safety

`MainDiffPane` and `ConflictView` own accepted pairs; there is no process-global
or thread-local pair cache. A task freezes source identity. Its completion may
publish only while the owning attachment/session/visit, selected file, frozen
revision and request identity still match. Keep main diff's publication/source
checks as well. A stale result cannot repopulate a hidden or replaced pane.

Retire on replacement, hidden-pane teardown or attachment departure, not only
when an old frame finally releases its entity. Retire PDF subscriptions at that
edge, preserve the focused body's identity for the existing host focus handoff,
and avoid stealing focus from unrelated input. Reuse immutable accepted content
only when source identity and compatible side shapes agree.

Preparation has distinct Loading, Ready and Failed presentation states.
Loading keeps the two role headers without an absence/error caption or
external-compare action. A failed Backend open relinquishes the active key
and permits one retry; a second failure stays Failed until observation,
selection or attachment retirement changes that lifetime. Repaints cannot
produce a retry loop. Absent sides and failed reads remain distinct.

Retirement advances a request generation even when the same file/revision is
immediately selected again. Cancellation is an optimization, not the stale
completion proof: publication checks the generation as well as owner, key and
current selection. Feature-only tests hold genuine worker completions and
deliberately let retired tasks finish to exercise those checks independently.
The first uncached render is also guarded by a thread-local Backend-open
counter; a cached second-frame repaint alone cannot establish off-thread I/O.

Choices remain derived from `ResolutionBuffer`; no viewer-side `chosen_side`
flag. Save continues to freeze revision-bound intent and stage the chosen raw
OID/mode, never preview/decoded bytes. Raw adoption offers one side, not Both.
Absent-side deletion requires the real #1734 domain/save path: neither an empty
blob nor an absent Current/Incoming choice is a deletion implementation. Keep
directory/file resolution intact when deleting obsolete raw helpers.

## Four-step delivery and regression gates

1. **① Preparation consolidation and ADR (this slice).** Bounded frozen-side
   backend input, one off-thread preparation entry and host-owned conflict
   results. No new labels, choices, cap/classification behavior or conflict PDF
   feature. Cover absent versus read error, frozen source/revision and stale
   completion/retirement. This ADR records the contract, not a passing result.
2. **② Shared renderer cutover.** Extend the diff model/helpers for the proposed
   pair contract; consolidate headers, placeholders and image geometry. Apply
   the decided shared 8 MiB/magic-first union policy. Add boundary, misleading
   extension, BMP, unequal-image-dimension, mixed-side and absent geometry
   coverage while preserving text/Result semantics and existing diff probes.
3. **③ Conflict feature completion.** Enable prepared PDF pairs with initial
   synchronization; integrate A+C per-side adoption/status/Change and AX names.
   Land/reuse #1734 before deletion acceptance. Exercise PNG/JPEG, PDF,
   binary/cap/decode/read failures, symlink/gitlink, text, mixed and absent sides;
   independently assert stage 0 equals the chosen OID/mode, or absence on deletion.
4. **④ Acceptance and documentation.** Record full regression and native
   evidence, update completion status/changelog, and close #1742 only when all
   content-kind/save requirements and the deletion dependency are satisfied.

**Required at every step, run by the integrating owner:** keep existing semantic
assertions unchanged except the explicitly approved step-② policy expectations.
Run the scoped native GUI suite with `KAGI_GUI_E2E=1`, `--features gui-e2e`,
`--test gui_e2e_runner` and an explicit `KAGI_GUI_E2E_ONLY` filter containing:

- Images: `binary_diff_prepared`, `binary_diff_owner_transitions`,
  `binary_diff_worktree_owner`, `binary_diff_linked_empty_revisit`,
  `diff_binary_states`.
- PDF: `commit_pdf_sources`, `commit_pdf_staged_rename`, `commit_pdf_owners`,
  `commit_pdf_continuous_sides`, `commit_pdf_fit_width`, `pdf_media_sources`,
  `pdf_media_bounds`, `pdf_media_symlink`.
- Synchronization/focus: `commit_pdf_sync_scrolling`,
  `commit_pdf_sync_lifecycle`, `pdf_focus_retirement`,
  `pdf_focus_retirement_preserves_input`.
- Conflict: **`conflict_`** (the whole registered conflict family, not only
  `conflict_image_zoom`), including save/directory-file boundary and lifecycle
  coverage, plus new preparation/source/PDF scenarios as each step adds them.
- Off-thread diff regression: `commit_diff_off_thread`.

The integrating owner also runs relevant backend/domain tests, workspace build
and tests, formatting, Clippy and repository invariant gates under AGENTS.md;
these are requirements, not results of this documentation change.

## UI acceptance contract and evidence limits

Step ① has no intended visible change; `docs/ui/modern-ui.md:70-76` therefore
does not require a new visual reference. Later structural work must follow its
full procedure (`80-140`): dense source headers/actions, open media bodies,
existing A+C/KagiButton/keyboard adoption and Editor Fit Width parts. Use
VS Code's side-by-side diff editor as the named product reference for aligned
source hierarchy, not its conflict-write semantics; record the actual reference
state/link or image before visual implementation, alongside the A+C design.

Specify rest, hover, pressed, focus-visible, disabled-with-reason,
selected/adopted/rejected, loading, absent, cap/unavailable and read/decode error
states. Keep role colors and scaled geometry. Capture PNG/JPEG/PDF Light/Dark
before/after at matched data, viewport, language, zoom and state, with the product
reference beside them. Exercise narrow/zoom-risk layouts and real keyboard,
focus, AX and relevant IME paths in Tier B; deterministic Tier A geometry alone
is not evidence for these. Record unobserved cases explicitly.

Step-① verification after review (2026-10-24): all 82 scenarios selected by
the image/PDF/sync/focus/`conflict_` plus `commit_diff_off_thread` filter passed,
with existing regression assertions unchanged. The preview scenarios cover
first-render Backend I/O, loading, poisoned-backend repaint, stale selection/
revision/retirement completions and bounded read retries. Both feature-only
render-probe unit tests passed. Workspace tests passed (3,776 tests, 48 ignored);
formatting, build, Clippy (only existing unrelated warnings), and repository
invariant gates passed. The two initial new refresh scenarios incorrectly
called the once-per-cycle detector; they now exercise real external reload
before asserting the changed observation, and the full filter was rerun green.

Tier B used the supplied plain `kagi-main` baseline and a fresh plain cutover
build, isolated copies of `merge-complex`, EN/Catppuccin/100% at 1392×883 logical
points (2× display). Native captures cover the original `image.png` bytes,
`vendor/sub`, and an identical valid-PNG fixture variant on both copies.
The main-pane pixels match exactly for all three before/after pairs, including
unchanged labels, image dimensions/byte captions and gitlink identities.
The original fixture's 25/26-byte PNG inputs do not decode; the valid-PNG
variant separately proves actual image rendering. This step does not claim
new controls, PDF conflict presentation, native AX/IME coverage or completion
of steps ②–④ or #1742.

## Steps ②–④ implementation amendment (2026-10-26)

The implementation extends `diff_view.rs` with `SidePair { left, right, headers }`
and independently typed side content. Existing diff source adapters and the
conflict attachment both call the preparation owner; neither rendering nor
adoption reads Git. Signature recognition is pure domain logic shared with the
backend conflict detector, so an all-ASCII PDF also takes the authoritative raw
resolution path rather than a lossy text Result. Recognized bytes outrank an
extension, and successful decode is distinct from merely recognizing a signature.

The common renderer owns source headers, body placeholders, metadata and image
geometry. Conflict alone supplies action rows. A decision is derived from the
buffer's raw OID/mode; Change clears that decision in `ResolutionBuffer`, preserving
the source identities and marking the unresolved draft for normal autosave.
No viewer-local chosen-side flag or alternative Save writer is introduced.
The raw Save pipeline and its contract log lines remain unchanged: the selected
OID/mode is staged, while worktree bytes and mode are intentionally left untouched
(#297/#298). Native acceptance compares both the exact stage-0 identity and that
unchanged worktree; any resulting index/worktree difference is not a new writer
behavior introduced by this viewer.

The accepted conflict preview owns its PDF pair. Conflict starts synchronized;
diff retains its independent initial state. Retirement immediately disables
callbacks and clears subscriptions even when an earlier frame retains cloned
viewer handles. The root's existing retiring-focus handoff also observes conflict
PDF bodies and does not steal unrelated input focus.

HIG design gate: the independent review rejected the initial sketch; Main approved
the revised matrix with final corrections recorded in
[the v3 proposal](https://github.com/TomiXRM/kagi/issues/1742#issuecomment-6413001918).
The source header has no controls; an aligned action row sits below it, using
existing NeutralTinted tool buttons and outline-only Used / Not used labels.
The unchosen body remains full contrast. PDF keeps the Editor toolbar and one
pair-level sync control in the file header, without a second shared size/OID
footer, matching the approved PDF figure. Its source metadata stays in the model;
the existing PDF viewer owns document status and errors. An extra fixed footer
can exhaust the narrow/high-zoom body allocation, clipping the document viewport
and routing body clicks to the footer instead. Other side kinds retain their
shared metadata summaries. External comparison belongs to a shared row outside
both source groups. EN headers use Current / Incoming.
Selectable symlink targets and full gitlink OIDs remain source content, not paths
to follow. The figure's contrast calculations cover Apple Light/Dark, not all
themes or native assistive-technology delivery.

`tests/gui_e2e/side_pair.rs` adds image/PDF Use → Save acceptance coverage, including
image Change → Use, with independently captured stage OIDs/modes/bytes and stage-0 oracles.
The final native run passed all 21 scenarios selected by `conflict_side_pair_`,
`conflict_delete`, `conflict_dir_file`, `binary_diff_prepared`, `diff_binary_states`,
`conflict_image_zoom`, `commit_pdf_`, `pdf_media_` and `pdf_focus_retirement`.
This includes the compact JA 167% PDF fit/focus regression. After integrating
#1734, the absent-side button freezes a deletion intent and defers it to the
existing application confirmation boundary. It never adopts an empty blob;
ancestor-only deletion keeps the single Remove from Index action. The real PNG
modify/delete scenario covers both absent sides in EN/JA, inert root Return,
Escape cancellation, unchanged preconfirmation index/worktree, and confirmed
absence of stages 0–3 with one successful receipt.

Native evidence for source commit `86bcfba9` is attached to
[#1742](https://github.com/TomiXRM/kagi/issues/1742#issuecomment-6423133480).
The plain build (features `[]`) used real Git merges with genuine PNG/JPEG/PDF,
isolated profiles and a 1392×883 logical viewport on a 2× display. EN Light and
JA Dark captures cover undecided/decided images, PDF toolbar/synchronized page
navigation, explicit fallback metadata and the modify/delete confirmation sheet.
EN additionally demonstrates independent PDF page navigation. Native EN and JA
Saves independently verify the selected PNG/PDF stage-0 OID/mode and unchanged
worktree bytes/mode. Physical root Return and Escape leave the pending deletion's
index/worktree unchanged; confirmed deletion execution is covered by Tier A.
EN Dark and JA Light add the malformed-image/binary/submodule/absent-side
cross-checks, completing the four visual combinations required by #1773.
All owned apps exited normally. These captures do not claim native VoiceOver/AX
announcements or IME coverage, nor substitute for independent final HIG review.
