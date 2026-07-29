# ADR-0135: Conflict editor redesign — bands, not checkboxes

- Status: Accepted
- Date: 2026-07-29

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
