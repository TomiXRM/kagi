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

### The body is seeded from `commit.template`

`Backend::commit_template()` resolves the `commit.template` config, expands a
leading `~`, reads the file and strips comment lines. It fills the body on first
open when there is no draft to restore.

Comments are stripped on **load**, not at commit time: kagi has no editor step
where git would strip them, so leaving them in would either commit them or make
the pane lie about what it will commit.

An unset or unreadable template is `None`, never an error — a stale
`commit.template` path is common and must not block committing.

### Trailers live in the body text

The co-author picker appends a `Co-authored-by:` line to the body rather than
maintaining a side list. The trailer *is* the message, so the draft file, the
plan modal and the existing `parse_coauthors` display path all keep working with
no extra plumbing, and the user can edit or delete one like any other text.

Candidates are distinct authors from recent history minus `user.email`, walked
on click rather than per frame (the panel re-renders at 60fps).

### Three icons instead of a pill toolbar

Sparkles generates a message, person+ adds a co-author, undo amends. Amend keeps
its full behaviour and its plan; only its full-width button is gone.

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
- The footer is five elements: subject, body, icon row, optional unstaged
  warning, button.
- `parse_coauthors` gains a second caller in spirit (the picker writes what the
  inspector reads), so the trailer format is now load-bearing in both directions.
