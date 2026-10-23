# Conflict sidebar — research, hypotheses and FRD

Status: design experiment for `feature/conflict-hunk`; not implemented or approved for main.
Date: 2026-10-23. Baseline: PR #1730, `81a5973b` (ADR-0227 R2).

## Requested change and process gate

Replace the Files-page round trip with a collapsible left file tree. Split that
sidebar vertically: work remaining above, saved resolutions below. Keep the
right side focused on one hunk or full Result, not every comparison at once.
Do not explain the workflow with paragraphs or a mandatory tutorial.

Required sequence: linked literature research → hypotheses/FRD → editable
OpenPencil design → Fable cross-review and revision → interaction experiment on
the reviewed design → GPUI implementation and real native verification.
Neither a screenshot nor a review alone permits skipping the later gates.
The existing R2 implementation is retained until this design passes them.

## Linked literature trail

| Source and followed connection | Evidence | Design consequence / limit |
| --- | --- | --- |
| [Apple Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars) → Split views / Layout / Disclosure controls | A leading sidebar supports navigation; grouping and hiding reduce space costs. Keep labels succinct; avoid critical actions at its bottom. | Show the file navigator initially, give it a persistent toolbar toggle and View-menu command; do not bury Continue below a long file list. |
| [Apple Split views](https://developer.apple.com/design/human-interface-guidelines/split-views) → Sidebars | Persistently highlight selection, constrain resizable panes, offer multiple ways to reveal hidden panes. | Selected file remains visible as selected while its content is on the right. A divider resizes the sidebar and its two sections. |
| [Apple Disclosure controls](https://developer.apple.com/design/human-interface-guidelines/disclosure-controls) → Outline views | Keep frequent actions exposed; use disclosure for related details. | File switching is frequent, so it should not be hidden behind Files. Hunk context remains optional. |
| [Apple Outline views](https://developer.apple.com/design/human-interface-guidelines/outline-views) → Split views / Search fields | Hierarchical files fit a scrolling outline; retain expansion, support column resizing and centered truncation. | Use an actual folder/file hierarchy; compact single-child folder chains, retain expansions, expose full paths. Do not build decorative indentation without tree behavior. |
| [NN/g Recognition and Recall](https://www.nngroup.com/articles/recognition-and-recall/) → Progressive Disclosure | Visible context helps recognition; unexplained icons and tutorial memorization are weak substitutes. | Keep pending and saved work locatable; short section labels plus file/status icons, not color alone. |
| [NN/g Progressive Disclosure](https://www.nngroup.com/articles/progressive-disclosure/) → staged disclosure | Staged disclosure is problematic when users must repeatedly alternate between interdependent steps. | Keep file navigation alongside editing; separate final review/commit, not the frequently reused navigator. This is a hypothesis for Kagi, not a measured improvement. |
| [VS Code merge conflicts](https://code.visualstudio.com/docs/sourcecontrol/merge-conflicts) → staging / operation completion | Merge Changes becomes Staged Changes after staging. Complete Merge handles one file, not the whole repository operation. | Distinguish draft completion, saved resolution, and operation completion structurally. Do not copy the ambiguous Complete Merge label. |
| [WAI-ARIA Tree View Pattern](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/) → focus versus selection | Arrow navigation, expansion and selection have distinct semantics; Enter can select when loading should not follow focus. | One Tab stop per tree, arrows navigate, Enter opens; explicit selected/focus states. Web APG informs interaction, not proof of native VoiceOver behavior. |

The earlier [actual Fork / GitButler / VS Code comparison](https://github.com/TomiXRM/kagi/issues/1729)
is runtime evidence. Its finding that saved Kagi files disappear motivates the
lower section. GitKraken remains user-approved deferred; no new claim of having
examined its conflict editor. Apple guidance does not prescribe two equal-height
file groups, and its usual two-level sidebar advice needs care with deep paths.

## Hypotheses and disconfirming observations

- H1: Persistent file navigation reduces repeated Files → Open detours. Reject if
  users still cannot locate another file or the sidebar consumes essential code space.
- H2: Moving a file below only after verified Save communicates completed file work
  without suggesting the merge is committed. Reject if users stop at all-saved or
  interpret a ready unsaved draft as saved.
- H3: One compact hide/show control makes the same layout usable with a terminal
  and narrow/high-zoom windows. Reject if restoring the tree is undiscoverable or
  hiding it changes the selected file, edits, or scroll state.
- H4: Stable path hierarchy and selected-row continuity reduce disorientation
  during Save. Reject if a row moves and users lose the file or act on another one.

These are design hypotheses, not empirical results. Model reviews are not human
novice tests, and multilingual models cannot establish cross-language usability.

## Information architecture and state meanings

Two sidebar sections, not three pages or three file groups:

| Internal condition | Sidebar placement | Short proposed JA / EN label | Right-side behavior |
| --- | --- | --- | --- |
| Untouched or partial draft; markers remain | Upper | 作業中 / In progress | Existing hunk choices or manual Result editor |
| Ready Result, not saved | Upper, small unsaved indicator | 未保存 / Unsaved | Review and explicit Save available; no completed checkmark |
| Save executing or verification/read acceptance pending | Upper | 保存中 / Saving | Retain exact Result; no optimistic move below |
| Accepted save for this operation/path identity and current staged resolution | Lower | 保存済み / Saved | Inspect the accepted saved Result; never mount stale editable source hunks |
| Save refused or failed | Upper | 未保存 / Unsaved | Preserve draft/selection; bounded error and durable Operation Log |
| External change makes saved evidence stale | Never claim current saved resolution | 要確認 / Review needed | Reconcile against current accepted read before enabling a write |

Section headers: `作業中 (n)` / `保存済み (n)`; EN `In progress (n)` / `Saved (n)`.
A file can have a ready unsaved draft without being an unresolved text conflict;
the upper section must therefore not label every child as an unresolved hunk.
A source-hunk count is not authoritative after manual editing or recovery.

The third boundary is operation-level, not a third file group: merge proceeds to
existing commit review, then explicit Commit. Rebase/cherry-pick Continue can
encounter another conflict; stash restore does not manufacture a merge commit.
Counts reaching zero must not by themselves authorize Continue.

## Layout and interactions

- Dense navigation, readable editing surface. Initial sidebar width hypothesis:
  248 logical px at 100%; resizable with bounded width. Do not change global density.
- Upper and lower groups have independently scrollable content and a thin resize
  divider. Start near 60/40 when both contain files, not mandatory equal halves.
  An empty lower group consumes its header plus a quiet dash, not half the window.
- Nested folders have disclosure controls and state counts only where useful.
  Single-child folder chains can be compacted; basename remains readable and the
  full path is available through tooltip/AX and the right-hand document header.
- Do not show unrelated automatically merged or pre-existing staged files as
  saved conflict resolutions. Keep stable path ordering within each section.
- Click/Enter opens a file in the right pane. It captures pending editor input
  through the existing navigation owner and never saves, stages, or commits.
- Save keeps the current file selected as it moves below; the right pane becomes
  a current saved-result inspection. Do not auto-open a different file underneath
  the pointer. The user chooses the next upper file explicitly.
- Lower files are inspectable. Re-editing saved content must use an existing
  supported edit/write route with a fresh approval; it must not resurrect retired
  conflict stages or be simulated by moving a row back up.
- Sidebar toggle remains in the workspace toolbar, outside the hidden region.
  Tooltip and AX names identify show/hide. View-menu/keyboard access provides an
  alternative. Hiding a focused tree returns focus to the toggle; editor focus
  stays with the editor when it owns focus. Selection/expansion/scroll survive.
- On compact windows, hiding is available without disabling file navigation.
  Keep a visible restore control and current path; do not introduce an invisible
  edge gesture as the only route. Mock both shown and hidden 940×660 states.
- Right-side hunk navigation remains local to the current file. Whole Result
  review/edit is still separate from the source-choice surface. Do not restore
  the old all-files × all-panes dashboard.
- Final operation action belongs above the right pane and hands off to existing
  operation/commit review. It does not live below the saved tree or run on Enter
  from a file row. At all-saved, right-side review remains meaningful, not a dead end.
- Terminal visibility is preserved; the sidebar toggle never closes it.
- No drag-and-drop between groups: it would incorrectly imply a visual move is
  sufficient to resolve/stage a file.

## State and ownership contract for implementation review

Existing evidence: `src/ui/conflict_view/flow.rs:20-33` captures pending Result on
navigation; `:119-145` derives readiness/manual divergence; `:147-181` routes
Overview/Hunk/Review. ADR-0227 currently removes saved paths after accepted read.

Extend `ConflictView` presentation and the existing session-owned accepted read;
keep `ResolutionBuffer` as the draft owner. Do not add a second `resolved: bool`
or a UI-only saved-file ledger. The saved tree requires evidence retained by the
operation/read lifecycle, scoped to operation incarnation and path identity,
with reconciliation after external changes and process restart. A historical
successful receipt alone does not prove today's staged bytes still match.

Before GPUI coding, identify which existing operation/read owner can supply the
current operation's conflict path set plus accepted staged-resolution identities.
If that information is absent, extend that owner and document the minimal contract;
do not infer completion from disappearance, marker absence, or an unrelated index
entry. Unknown recovery identity must remain explicit rather than falsely saved.
A current operation with zero remaining conflicts must retain no stale writable
buffer merely to keep the lower tree populated.

Required states: rest, hover, pressed, selected, focus-visible, disabled with
reachable reason, loading, empty, error, and stale/replaced operation. File-row
focus and selection are separate. Folder arrows never mutate content. Save and
Continue retain plan → confirm → preflight → execute → verify → oplog, writer
admission, revision checks, one-modal ownership, and IME Enter/Escape protection.

## OpenPencil boards and review protocol

Build editable layers, not a flattened screenshot. Include named boards:

1. Mixed state: three upper files, one lower; selected partial text conflict.
2. Ready unsaved: same selected file in upper, full Result review and Save.
3. Save accepted: file moves below, remains selected, saved Result inspection.
4. All saved: upper empty, lower contains files, operation review is reachable.
5. Sidebar hidden: unchanged file/content, explicit restore control.
6. Narrow Apple Dark: shown and hidden, terminal open, no unreachable actions.
7. Failure/stale: unsaved draft retained, no false lower-section success.
8. Many/deep paths: scroll, compact chains, same basename in distinct folders.

Fable cross-review must inspect the actual OpenPencil document/exports and the
FRD. Review information hierarchy, file/draft/save/commit confusion, compact
layout, selected-row continuity and keyboard discoverability. Record findings,
change the design, and resolve findings before interaction trials. Do not silently
substitute another model if Fable cannot be selected.

Then run goal-only mock tasks: switch files without losing a draft; find the file
just saved; hide/reveal the navigator; find another same-named file; distinguish
all-saved from committed; recover from failed Save. Log intended target, actual
hit, selected path, group membership and next action. Input-coordinate errors
are separate from UX interpretation errors. Mock transitions do not prove Git
writes, persistence, safety, rendering performance or native accessibility.

## GPUI acceptance gate after mock review/trials

- Retain drafts, selection and tree expansion through switches, hide/show, Save,
  refresh and operation replacement. No mutation from navigation or folder controls.
- Partial/ready-unsaved stay above. Only current verified saved resolutions appear
  below; external changes and unrelated staged files cannot fake completion.
- Save refusal preserves data and position. Empty manual result is intentional,
  not equivalent to missing draft; special kinds remain on safe existing routes.
- Saved selection opens current saved bytes. Last Save reaches existing merge
  commit review; no auto-commit, no stale-buffer recreation, no wrong-operation write.
- Keyboard tree/toolbar/editor ownership, EN/JA, Apple Light/Dark, 100%/167%,
  normal/narrow windows, long paths and many rows are exercised in native GUI.
- Real Save/Continue/Commit output is independently checked against exact bytes,
  index stages, parents and active operation. Measure navigation/list behavior
  before making performance claims. Attach before/reference/after screenshots.

OpenPencil review and mock experiments are prerequisites, not substitutes for
these native acceptance checks. Main stays unchanged throughout this experiment.

## Record: OpenPencil design, Fable review, mock trials (2026-10-23)

Design source: `docs/ui/conflict-sidebar/build-boards.js` (run through
`openpencil import` + `eval`, see `rebuild.sh`). Fonts: Noto Sans JP, JetBrains
Mono, Noto Sans Symbols 2 fetched by OpenPencil; Inter alone drops CJK glyphs.
Thirteen boards: 01 mixed, 02 ready-unsaved, 03 save-accepted, 04 all-saved,
05 hidden, 06a/b narrow dark, 07/11 save-refused EN/JA, 08 deep paths, 09/10
other files opened, 12 commit review hand-off.

Fable cross-review (P1/P2/P3) changed the design before any trial:

- One primary per screen: toolbar Continue is outline while 作業中 > 0 and the
  only primary on the all-saved board. Next-file after Save is a secondary
  control beside the title, never under the pointer position of the Save button.
- No banners. All-saved state is carried by `作業中 (0)`, `保存済み (n)`, the
  `未コミット` chip beside the operation and `コミットを確認`.
- Stale evidence after an external change renders as `要確認` with `保存済み (0)`
  and an unavailable Continue; no green chip survives a changed index.
- Empty group collapses to header + dash; the lower group is content-sized up
  to 40 %; an explicit scroller thumb marks clipped rows; folder counts only
  when collapsed; neutral chips at 11 px with ≥4.5:1 ink; tree rows at the
  existing 20 px; focus ring distinct from selection; `すべてリセット`.
- Narrow JA adoption keeps the verb: `● 採用` / `○ 採用`; narrow toolbar keeps
  その他; toggle shown = grey wash, hidden = surface + count badge; tooltip layer
  `サイドバーを表示 ⌘⌥B`.

Click-through trials (goal-only, Vietnamese goals, Haiku 4.5 / Sonnet 5.5 /
GPT-6 Luna, hit regions from the actual node geometry):

| Observation | Round 1 | Round 2 (after revision) |
| --- | --- | --- |
| Recovered a refused Save without losing the draft | 0/3 (Reload avoided: "might discard the draft") | Haiku ✓, Luna ✓, Sonnet named `もう一度保存` as the retry but stopped on two prototype no-ops |
| Located the just-saved file before clicking | 3/3 | 3/3 |
| Stated "no commit yet" with on-screen evidence | 3/3 (`未コミット`, lists, `コミットを確認`) | 3/3 |
| Revealed hidden sidebar and opened another file | reveal 3/3; open blocked by missing boards | Sonnet ✓, Luna ✓ (Haiku did not run D2) |
| Out-of-range / off-target clicks | Haiku 19/37, Luna 1/8, Sonnet 1/10 | Haiku 3/17, Luna 0/7, Sonnet 1/8 |

Design consequence adopted: after a refused Save the screen keeps one primary
`もう一度保存` / `Save again` (Save already re-reads inside plan → preflight) and
a secondary `操作ログ`; the separate Reload control is removed. Prototype
artifacts, not design findings: inert Operation Log affordances, an END state
without a screen, and (round 1) missing boards for other files. Aim errors
stay separate from UX interpretation; they motivate the harness driver, not
larger controls. Model trials are not novice-human evidence.
