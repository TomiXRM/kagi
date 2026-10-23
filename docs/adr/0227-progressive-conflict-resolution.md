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
framework. Keep a persistent, collapsible 248px file tree beside focused
Choose/Review pages rather than place a tutorial over the old layout. There is
no Overview/Files page or Open control; initial selection opens the first file
directly in Choose.

| Surface | Primary content | What is not permanently shown |
| --- | --- | --- |
| File tree | In progress and Saved groups, stable paths and selected state | Unrelated staged files |
| Choose | Selected hunk, whole-file source panes, resulting snippet | Full Result |
| Review | The authoritative full Result and manual editor | Persistent source comparison |

Choose compares the whole file in two scrollable source panes. Each pane has a
fixed height of twelve code rows at 100 % zoom, or eight when the window is less
than 1000 logical pixels wide. The current hunk is highlighted and scrolled into
view in both panes; Previous/Next changes the hunk and re-scrolls both sources.
There is no Context toggle or content-sized source-height clamp. A/B width
remains resizable; the obsolete vertical split is removed. The decision page
itself scrolls when a narrow/high-zoom window and the terminal leave insufficient
height. Next becomes Review on the last hunk.

The adoption controls use Xcode's positional convention: Use Left, Use Right,
and Use Both with explicit Left First / Right First menu choices. Japanese uses
左を使う、右を使う、両方を使う and 左→右 / 右→左. Button labels contain no side-dot
glyphs; accessible names append the actual side identity. Pane headers retain
their current/incoming role, side marker and branch/commit identity. Adoption
never automatically advances. Edit beside Result opens the existing full-file
editor before all hunks are decided.

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
revision-bound save route and a secondary Operation Log action. It adds neither
a Reload control nor a banner; successful Save clears refusal presentation.

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
- While Result diverges, source adoption must not silently replace manual edits.
  Comparison and navigation retain the Result; the existing guarded Reset is
  the explicit discard route. Mutation handlers recheck this condition.
- The Use Both order menu adopts both sides as a single draft choice. Existing
  low-level order controls only reorder already adopted content; an unchanged
  order is not an edit.
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
