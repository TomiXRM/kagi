# ADR-0134: A simpler commit pane

- Status: Accepted
- Date: 2026-07-26

## Context

User report: Zed, VS Code and GitHub Desktop all commit through a plainer
surface than kagi's, with the same feature set or more. kagi's commit footer had
accumulated eleven stacked elements above the button:

- a three-line staged preview (`N files staged` / `+2 ~1 -1` / `→ branch` /
  `by Name <email>`)
- a `Commit message` label and a `⇄ Template fields` mode toggle
- either one input, or six stacked template inputs in their own scroll box
- a `Suggest` pill, a `Lang: EN` pill, an `Enable Local LLM…` pill,
  a `● Local LLM available` line and a transient status line
- an unstaged warning
- the commit button, whose disabled state restated why it was disabled
- a full-width `Amend last commit…` button

The requested shape is what git itself has: a subject, a body, and a small row
of actions.

## Decision

### Subject + body, replacing the single field and the template mode

`CommitPanelView` holds `title_input` and `body_input` instead of `commit_input`
plus `commit_template_mode` and six `commit_template_inputs`. The pure
`split_title_body` / `join_title_body` pair in `kagi_domain::message` is the
only crossing between the two-input UI and the one-string message that drafts,
plans, amend and the oplog all consume.

The six-field template mode (`type/scope/summary/body/test/risk`, T-COMMIT-009)
is removed. It existed to impose structure on the message; the user's own
`commit.template` now does that job, and carrying both meant two authoring modes
to keep working.

Drafts written by older builds in `mode=template` stored their *expanded* plain
text (ADR-0042), so splitting them on load restores them correctly. No migration.

### The body is seeded from `commit.template`, comments and all

`Backend::commit_template()` resolves the `commit.template` config, expands a
leading `~` and reads the file **verbatim**. It fills the body on first open
when there is no draft to restore.

Comments are stripped at **commit** time, not on load — exactly where git strips
them. The first implementation stripped on load, on a "what you see is what you
commit" argument. That was wrong about what templates actually contain: the
common template is *entirely* comments (an emoji or commit-type cheat-sheet the
author reads while writing), so stripping on load produced an empty body and
looked like the template had failed to load. Reported as exactly that.

So the message exists in two forms, and the split is load-bearing:

| | includes comments | used by |
|---|---|---|
| `effective_commit_message` | yes | the draft file (must round-trip the template) |
| `committable_message` | no | commit, amend, "is there a message yet?" |

The "is there a message yet?" check uses the stripped form so that a body
holding only a cheat-sheet still counts as empty — otherwise ✨ would refuse to
fill it in. For the same reason a generated body is inserted *above* the
retained comment block rather than replacing it.

An unset or unreadable template is `None`, never an error — a stale
`commit.template` path is common and must not block committing.

### Trailers live in the body text

The co-author picker appends a `Co-authored-by:` line to the body rather than
maintaining a side list. The trailer *is* the message, so the draft file, the
plan modal and the existing `parse_coauthors` display path all keep working with
no extra plumbing, and the user can edit or delete one like any other text.

Candidates are distinct authors from recent history minus `user.email`, walked
on click rather than per frame (the panel re-renders at 60fps).

### One button in both states

The commit button is a single `Button` with `.disabled(!can_commit)` rather than
a `Button` swapped for a styled `div`. Two different elements meant the footer
changed height the moment a message was typed, and the disabled `div` used
`theme().surface` — the footer's own background — so it read as a ghost. Same
class of bug as the header buttons fixed in #220.

### Placeholders follow the UI language

`InputState` bakes its placeholder at construction and exposes no setter, so a
language switch left stale-language placeholders on screen. The panel records
which language its inputs were built with and rebuilds the pair (carrying the
text across) when that changes. Rare enough that the lost caret does not matter.

### Three icons instead of a pill toolbar, inside the body box

Sparkles generates a message, person+ adds a co-author, undo amends. Amend keeps
its full behaviour and its plan; only its full-width button is gone.

The icons sit *inside* the body's box (GitHub Desktop's placement). The box is a
plain div wearing the Input's border and background, holding an unstyled `Input`
and the icon row stacked under it. Two alternatives were rejected: `Input::suffix`
pins the icons to the vertical centre of a tall multi-line box, and absolute
positioning would let a long body run underneath them. A real stacked row cannot
overlap by construction.

### ✨ always overwrites

Generating used to refuse when the message was non-empty and report "message
not empty — kept your text". Clicking the button is an explicit request, so the
silent refusal read as the button being broken (user report). Both the
rule-based and LLM paths now replace whatever is in the inputs.

The generation-counter guard stays: a result whose `gen` no longer matches has
been superseded by a newer click and is still dropped. Only the
"is it still empty?" check is gone.

### The Template toggle

A `commit.template` is opt-out per user, not per repo: the toggle at the bottom
left of the body box adds or removes the template block, and the choice is
persisted (`commit_template_enabled`, absent = on). It is only rendered when the
user actually has a template configured.

Its on/off state is *derived from the body text* (does it contain a comment
line?) rather than tracked in a flag, so it cannot disagree with what is
actually in the box after the user edits or deletes the comments by hand. The
raw template is kept on the entity so switching back on can restore it.

### Text selection is an accent tint, not the row-highlight colour

`gc.colors.selection` was `theme().selected` — the list-row highlight, which is
deliberately a near-background neutral. Against an Input's background that read
as no highlight at all (user report). It is now the theme's accent at alpha 0.30,
matching the cap gpui-component applies to its own bundled themes; the selection
is painted *under* the text, so legibility is unaffected. A test asserts the
composited tint differs from the input background for every theme.

This is app-wide, not commit-pane-specific — every `Input` was affected.

### What was deleted, and what took over its job

| Removed | Where it went |
|---|---|
| `N files staged` / `+2 ~1 -1` | the staged section header already counts |
| `→ branch` | onto the button: **Commit to `main`** |
| `by Name <email>` | nowhere — it is `user.email`, not per-commit information |
| disabled-button reason text | the cause (nothing staged / no summary) is already on screen |
| `Lang: EN` toggle | `smart_commit_lang` now defaults to the **UI language** |
| `⇄ Template fields` + six inputs | the user's `commit.template` |

The Lang pill was the only way to change generation language, so removing it
alone would have stranded the setting on English. An unset `smart_commit_lang`
now follows `i18n::lang()`; an explicitly saved value still wins.

## Consequences

- Generated messages are `Style::Plain`. Template mode was what selected
  Conventional Commits, and nothing replaced that selector. Conversely
  `want_body` is now always true — there is always a body input to fill, where
  before it was only requested in template mode.
- The footer is four elements: subject, body (with the template toggle and icon
  actions inside it), an optional status/warning line, and the button.
- The transient smart-commit status is its own full-width line. Inside the
  right-aligned icon group it pushed the icons sideways as the text appeared.
- `parse_coauthors` gains a second caller in spirit (the picker writes what the
  inspector reads), so the trailer format is now load-bearing in both directions.
