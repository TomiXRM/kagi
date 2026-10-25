# ADR-0227: Progressive conflict resolution

- Status: Experimental on `feature/conflict-hunk`; not approved for main
- Date: 2026-10-23
- References: #1729, #1728; ADR-0057, ADR-0068, ADR-0071, ADR-0135, ADR-0197

## Problem

The existing conflict workspace simultaneously shows the file dashboard, two
complete source files, Result, and operation controls. Real GUI comparison with
Fork, VS Code and GitButler exposed ambiguous navigation, premature-looking
completion, and an ordering action that could adopt an empty result. At
940×660 and 167% zoom the old layout leaves almost no visible code.

The user rejected compensating with explanatory text. The intended audience
includes someone who cannot comfortably read the interface language: structure,
visible choices and immediate consequences must carry the interaction. Short
labels remain necessary; unlabeled icons and color alone are not substitutes.

## Decision

Extend the existing `ConflictView`; do not create another resolver or onboarding
framework. The binding visual contract is the 2026-10-23 Visual design pass in
[`conflict-sidebar-frd.md`](../ui/conflict-sidebar-frd.md#visual-design-pass-2026-10-23-after-the-user-rejected-the-wireframes),
implemented from `docs/ui/conflict-sidebar/build-hifi.js` (H1–H7), as amended by
the final 2026-10-24 live-build review Decision in that document.
It supersedes the earlier Choose/Review wireframes, detached adoption row,
Preview/Edit segments and gutter choice pill.

The workspace is a document: two borderless whole-file source panes above a
directly editable Result. Source jump bars identify each side; commit footers
identify their revisions. The left footer owns `‹ n/m ›` Previous/Next navigation.
Sources and Result divide available height responsively (2:1), replacing the
earlier fixed 8/12-row wireframe. Source rows are 24 logical pixels at 100% zoom
to fit the S=24 hunk controls without overlapping adjacent one-line hunks;
source viewports flex to the remaining space and scroll to reveal the selected
hunk in both sources. No Context toggle or content-sized source-height clamp is introduced.

The [FRD A+C decision](../ui/conflict-sidebar-frd.md#ac-per-pane-action-rows-2026-10-24-owner-mock-conflictacdchtml)
supersedes the gutter arrows and More menu with an action row above every hunk
in both source panes: unresolved rows offer neutral-tinted Use this side and
Both in that pane's block order (#1766); resolved rows show Used / Not used /
Partly used and Change, which resets that hunk. Action rows are **24px**, correcting
the mock's 26px to the uniform-list contract: both sources and the gutter retain
equal row counts and measured row heights. The gutter is a **control-free 12px**
hunk band with a focused-hunk edge. Result's header owns focused-hunk Reset
(enabled only for a resolved hunk) and Reset all with the existing two-step
confirmation; navigation, dismissal and focus departure retire that approval.
The file header shows `{m} conflicts · {k} resolved` (singular `1 conflict`) or
`conflict {m} 件 · {k} 解決済み`; footer navigation remains unchanged and adoption
never advances automatically. Per-line checkboxes, same-pane drag painting,
Space toggling and Shift+Up/Down painting retain their existing behavior and
localized side/line names. `ConflictView` and `ResolutionBuffer` remain the only
owners; there is no detached adoption row or Edit button.

Result uses the existing authoritative input directly, with left/right/manual
origin stripes, 18px code rows and an overlay scroller. Navigation
captures live input without changing its draft owner. A retained logical
Review transition does not switch to a second visual editor or hide sources.

The 232px sidebar is a source list with unpinned In progress and Saved sections
flowing top-down in one scroll owner. Section counts are trailing; empty sections
are headers only. Rows are 24px with folder/document icons and C/M/! state
letters. Selection uses focused/inactive fill without a focus ring.
The panel-colour file header has a sidebar toggle (grey wash when shown,
plain inline pending count when hidden), path jump bar, status and exactly one
file action: Save while editing, Next file in progress after saving. Saved
inspection has a lock and read-only code. The unified toolbar retains its right
icon group and compact operation strip. Push buttons use 20px height, radius 5
and 12px labels, with the header action aligned at the 16px trailing inset.

The upper In progress group includes partial and ready unsaved drafts. The lower
Saved group is derived from accepted `ConflictSession.resolved`, not a UI ledger.
Save retains the selected path when it moves below; the right pane inspects
current saved bytes read-only without remounting retired source hunks. Next file
in progress explicitly opens the first pending path; saving never auto-selects
another file. Empty groups retain a quiet header rather than a success banner.

The toolbar always retains Continue (Review commit for merge), secondary while
pending files remain and primary when none remain. Its existing guard still
admits ready unsaved drafts and special conflict kinds; counts do not authorize
writes. Sidebar hide/show retains selection and drafts, with a persistent toggle
and the existing View command. Arrow keys rove focus across both groups;
Enter/Space opens the focused row without saving.

Save refusal retains the editing path and exact draft. Result review shows
Unsaved · save refused with one primary Save again action through the same
revision-bound save route. Operation Log remains in the unified toolbar rather
than becoming a second file action. It adds neither a Reload control nor a
banner; successful Save clears refusal presentation.

The role is a focused editing workspace: compact navigation, readable code,
existing themed controls and one primary action for the current page. No
instructional paragraphs, permanent warning wall, new global spacing tokens,
or mandatory introductory carousel.

## Ownership and transitions

- `ConflictView` owns presentation page/disclosure state, `selected_saved` and
  `sidebar_hidden`; existing selected file and hunk owners remain authoritative
  for pending-file navigation. Saved-byte caching is read-only presentation, not
  evidence of completion. Navigation captures pending Result input.
  Page controls retire hidden editor focus to a retained navigation owner;
  hiding a focused tree returns focus to the toggle without stealing editor
  focus. Tab must work after pointer navigation without test-only refocusing.
- `ResolutionBuffer` and `HunkModel` own draft bytes and choices. Page transitions
  do not select, save, stage or rebuild the draft.
- Draft existence is not completion. Readiness follows actual Result and marker
  state, not whether any one hunk was selected or which page was visited.
- A manually edited Result may differ from the source-choice assembly. Derive
  that difference from actual text, including empty results, without a shadow
  manual flag. A shared borrowed domain traversal compares without allocating
  an assembled file. Identical text with different provenance is not divergence.
- While Result diverges, source adoption and per-hunk Reset/Change are disabled,
  including mutation-handler guards. Comparison and navigation retain Result;
  only the existing confirmed Reset all is the explicit discard route.
- The per-pane Both choices adopt both sides as a single draft checkpoint.
  Repeating the same choice is not an edit. Low-level order mutations still only
  reorder adopted content; focus and navigation do not adopt anything.
- Ordinary text add/add conflicts use the same existing materialization and
  hunk/manual editor. Binary, symlink and gitlink conflicts remain raw choices;
  changing presentation never authorizes lossy text materialization.

## Git safety and completion

Save/Continue/Commit keep ADR-0068 responsibilities and the existing frozen
revision, owner/visit and writer-lease checks. Save stages the explicitly reviewed
file. The file then leaves the authoritative unmerged-file list. Each accepted
operation read recomputes `ConflictSession.resolved` from the current operation's
`MERGE_MSG` conflict path list intersected with index stage-0 paths, minus all
currently unmerged paths. Unrelated automatically merged or pre-existing staged
paths never enter Saved. Stash has no such durable conflict-path evidence, so
its Saved group remains empty.

This is current accepted-read evidence, not remembered successful saves or an
OID history. A stale/replaced operation read must not retain a Saved claim;
reconcile with the new accepted observation and show Review needed while current
evidence is unavailable. Read-only saved inspection does not authorize a write.

A fully staged merge still needs an explicit route to its commit. Use the same
fresh `Detected` payload and accepted-observation checks for a merge with no
remaining unmerged files. Do not retain an old editable buffer merely to show
Saved. Preserve the existing ready-to-commit log contract. Sessions still
accept operation observations only through the accepted read model.

Continue hands off to the existing CommitPanel. Its existing merge-pending
presentation and reload precedence remain the owner of that handoff. The
experiment must not re-open the conflict workspace over the commit message or
silently commit when navigating pages.

## Evaluation contract

Use native Kagi on real isolated conflict fixtures, then revise observed
failures. Compare Japanese/English, Apple Light/Dark, ordinary and narrow/high-zoom
windows. Exercise partial selection, both orderings, manual editing, backtracking,
add/add, actual Save and Continue/Commit, and long file lists.

Keep the existing conflict scenario registrations (57) while extending their
oracles: per-pane Use/Both/Change, focused-hunk Reset and confirmed Reset all,
per-line checkboxes and same-pane range painting, footer navigation, direct Result
editing and writer-busy refusal, unpinned groups, hidden inline count, one header file action, and
byte/receipt-identical Git navigation.
Native pixel probes sample row padding outside glyphs and badges, in logical
window coordinates converted once by the window's scale factor. Renderer-fed
AX/focus probes do not establish native VoiceOver, IME or real-window usability.

The direct Result viewport uses layout-transparent measurement: an auto-sized
test wrapper must not collapse its flex height or redirect its click. Sidebar
rows attach the shared virtual-list arrow actions as well as pointer activation;
Enter/Space then open the focused pending or saved row. The hunk resolver still
has one roving Tab stop, with Down reaching the next hunk after source adoption
changes the entry point. Selected-hunk navigation strictly centers the hunk,
not a preceding context row that may fill a narrow/high-zoom viewport by itself.
Footer navigation retires a button's focus before it becomes unavailable, and
inline Up overrides the shared previous-row binding to return to its own action row.
Toolbar fitting charges the operation strip only to the right column; compact
conflict chrome lets repository identity truncate before losing right-hand
controls. Save refusal
coverage uses a ready draft with an externally stale revision; an unresolved
draft's unavailable Save is not an executable refusal path.

Blind model trials receive only the desired business result and native-window
input tools, not source, a design explanation or a suggested click sequence.
Record observed actions, screenshots and independently verified repository bytes.
A model's ability to read Japanese or reason about Git is not evidence that a
novice human or a person unfamiliar with Japanese can use the interface.

### First two native iterations

R1 separated pages but hid manual editing inside Review, showed too little
context for shipping choices, and reserved excessive vertical space. R2 exposes
Both/Edit, restores local context, and removes the empty resolved-files section.
Sonnet completed both real fixtures; independent verification found the exact
expected three blobs, merge parents and message, with a clean index/worktree.
Luna did not complete either trial; R2 reached manual editing and Save but omitted
the requested pickup contribution and left an extra blank line. Haiku's runs
were coordinate-confounded: many claimed control clicks landed above the visible
targets. Its “unresponsive UI” report is not established by those actions.

These results do not establish cross-language novice usability or prove R2 is
universally better. Remaining limits include tiny inline adoption targets,
full-file manual-edit navigation, and the need to understand whether combining
both alternatives is semantically valid. Preserve those failures in the
experiment report rather than compensating with more explanatory paragraphs.


## References

- [Apple: Onboarding](https://developer.apple.com/design/human-interface-guidelines/onboarding): teach through interaction; minimize prerequisite instruction.
- [Apple: Disclosure controls](https://developer.apple.com/design/human-interface-guidelines/disclosure-controls): essential controls first; related details on demand.
- [Apple: Undo and redo](https://developer.apple.com/design/human-interface-guidelines/undo-and-redo): reversible exploration and visible outcomes.
- Real-product evidence and its limits: [#1729](https://github.com/TomiXRM/kagi/issues/1729). GitKraken conflict UI was not evaluated because its license gate was deferred; do not attribute fresh findings to it.
