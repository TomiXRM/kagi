# ADR-0135: Conflict editor redesign — bands, not checkboxes

- Status: Accepted
- Date: 2026-07-29

Experimental amendment: [ADR-0227](0227-progressive-conflict-resolution.md) on
`feature/conflict-hunk` separates Files, one-hunk decisions, and full Result
review. It retains the source adoption bands but replaces the simultaneous
dashboard/full-source layout and adds direct Both/Edit actions. It is not a
mainline approval of that experiment.

## Context

User verdict on the hunk-level Conflict Editor: "かっこよくない". Specifically:
Apple software does not use small checkboxes as selection controls, there was
no syntax highlighting anywhere in the conflict surfaces, and the same
information/actions appeared in up to three places at once.

The editor's selection model (ADR-0071's file/hunk/line tri-state) is sound and
untouched — this ADR changes only the control surface and presentation.

## Decision

### Accent bands instead of checkboxes (Xcode's idiom)

- **Hunk header click** takes/releases that whole side of the hunk. The header
  is the control; the ☑/☐/— glyphs are gone.
- **Line click** toggles that single line — per-line selection survives (a
  deliberate departure from Xcode, which is hunk-only) but without a checkbox
  column: taken lines carry a solid 3px accent band and a translucent
  accent-tinted background; untaken lines lose the tint and render at
  0.22 opacity. First attempt used 0.4 — the rejected side still read almost as
  clearly as the taken one; the tint came later for the same reason ("bright vs
  dim" alone did not say *where the conflict is* — colour does, matching the
  diff views' coloured-row idiom).
- **"Use all" pill** in each pane header replaces the file-level checkbox.
- Sides keep their colours everywhere: Current = `color_branch`,
  Incoming = `color_remote`, in the bands, tints, pills and dashboard chips.

### Syntax highlighting in all three panes

A/B rows go through the same tree-sitter + per-theme pipeline as the diff
(ADR-0133), with the same combine-once → distribute-spans-per-row approach.
Cached per `(path, theme slug)` on the entity: row *text* never changes within
a session — selection clicks only flip `taken` flags — so clicking never
re-parses. A theme switch does.

### Result pane: one CodeEditor, `disabled` toggled

Preview and Edit were two different renderers (custom rows vs the CodeEditor
InputState) and their font and size drifted — twice: first the font (fixed by
the #219 wrapper-cascade trick, disproving an old comment claiming the cascade
"does NOT reach" the InputState), then the size. The fix for the class of bug,
not the instance: Preview *is* the Edit CodeEditor with `disabled(true)`.
`disabled` only skips the interaction handlers — background aside, colours are
untouched — so the highlighted, line-numbered view is identical in both modes.
This deleted the custom preview renderer and its content-hash highlight cache
the same day they were written.

The editor is created with `code_editor(lang_for_path(path))` instead of
`"text"`, which is what actually turns highlighting on.

### Duplicated chrome removed

| Was | Now |
|---|---|
| Persistent banner under the header (op summary + n/m) | gone — the dashboard header and the editor toolbar already say both |
| Dashboard: header / two large role-badge boxes / counts (3 bordered sections) | one compact block; sides are ●-chips colour-keyed to the bands, role words in the tooltip |
| File cards: "…" + open-externally + copy-path icons | "…" only — the same actions were in the card, the "…" menu *and* the editor toolbar |

### Order labels: English in both languages

`現在の側を先` was harder to parse than the English it translated (user:
"アホみたいな多言語対応"). Both languages now show
`Current → Incoming` / `Incoming → Current` — the arrow states what "first"
means: the order the two blocks land in the result when both sides are taken.

## Consequences

- ~100 lines of one-day-old preview-highlight code deleted; the conflict
  surfaces now have zero bespoke text rendering in the Result pane.
- The highlight cache lives on `ConflictView` as an `Rc<RefCell<…>>` threaded
  through `EditorChrome` (the `geom` cells' precedent) — interior mutability
  because it is filled from the render path.
- Line-level selection now requires clicking the line itself; there is no
  neutral "focus this hunk without changing it" click on hunk rows. Prev/next
  in the toolbar still focuses without mutating.
- The banner's one unique service — reminding that a merge is still in progress
  on the commit-pane screen (`conflict_merge_pending`) — is now covered only by
  the seeded "Merge branch …" commit message. Acceptable; revisit if users
  commit-then-wonder.

## Amendment — 2026-10-12: keyboard and accessibility adoption (#1311)

The accent-band header and code-line presentation remains, but each adoption
target is a named Button, not a click-only row. A hunk RowList is one Tab stop;
Up/Down moves between hunks, Left/Right traverses that hunk's side, both-order
and line controls, and Enter/Space uses the same activation listener as a click.
File-side Use all controls are separate named Buttons. Focus-visible uses the
shared fixed-width ring, without changing the syntax highlighting or dimming.

Visual captions and accessibility names use the operation-dependent side roles
and actual sanitized identities from `mode.labels()` (ADR-0058), not raw
ours/theirs. EN/JA names include the relevant hunk or line number. Adoption
describes None/Partial/All and exposes toggled state for All. Order controls
instead name the requested first side and describe the currently selected
output order; they change only the existing LineOrder setter and do not
implicitly adopt any unselected line.

Roving ownership is rebuilt when the selected path or its context/hunk/line
structure changes, including consecutive rebase/cherry-pick steps on the same
path. Domain selection, undo/redo and the Save/Continue/Commit separation
(ADR-0068/0106) are unchanged.

## Amendment — 2026-10-14: file-bound Reset and Result segments (#1511/#1512)

Reset all keeps the existing two-stage interaction, but approval belongs to the
currently edited path. File navigation, selection changes and leaving the reset
control's focus scope retire the armed stage; a busy writer cannot arm or reset.
Returning to a file never restores an earlier approval. This is buffer/draft
editing only, not a new Git operation or confirmation manager.
Keyboard activation retains focus on the actual Reset Button inside that scope,
so the second Return/Space reaches the same final-confirm listener and the
focus ring remains visible. Only activation from outside enters the scope.

Result Preview/Edit uses the shared `TabList`, as the historical Inspector does
(#1495), with explicit Preview=false and Edit=true activation. Re-activating the
selected segment is a no-op: it does not resynchronize inputs or alter manual
text. The surrounding nonbutton area has no activation. The same existing mode
owner drives selected fill, accessibility state and the rendered Result body,
and writer-busy refusal remains in the mode setter.
