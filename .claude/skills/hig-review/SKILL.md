---
name: kagi-hig-review
description: Apple Human Interface Guidelines gate for every change to what the user sees in Kagi. Use before implementing a new control or layout (placement decision) and again before merging (review of the real-window screenshots). An independent reviewer quotes the HIG text it relies on; a UI PR does not merge with an open P1/P2 HIG finding.
---

# Kagi HIG review gate

Canonical source: `.claude/skills/hig-review/SKILL.md`. Codex reads it through
`.agents/skills/kagi-hig-review`. Owner ruling (2026-10-25, #1764): Kagi is
developed against Apple's Human Interface Guidelines; a control in a place the
HIG does not sanction is a bug, whatever its tests say.

## When it applies

| Change | Gate |
|---|---|
| New control, new placement, new screen, moved control, new sheet/alert/menu | **Design review before implementing** (step 1) and **merge review** (step 2). |
| Size/spacing/colour-role/hierarchy change of an existing part | Merge review (step 2). |
| New state of an existing part (disabled/loading/error), keyboard path, AX name | Merge review limited to that part. |
| Wording only, no visible change | None. |

This gate is in addition to `docs/ui/modern-ui.md` (role, density, reference,
state contract) and the Tier A / Tier B verification in
`.claude/skills/verify/SKILL.md`. It replaces nothing.

## The reviewer and the corpus

The review method and the HIG text come from the owner-chosen
[`dickwu/apple-design-skill`](https://github.com/dickwu/apple-design-skill)
(pinned `914b6a3`, 2026-10-15). It is **not vendored**: the repository carries
no license and `references/hig/*.md` is Apple's text. Install it per machine
and point reviewers at it:

```sh
git clone https://github.com/dickwu/apple-design-skill.git ~/.claude/skills/apple-design
git -C ~/.claude/skills/apple-design checkout 914b6a3
```

A reviewer follows `~/.claude/skills/apple-design/SKILL.md`: load the always-load
set (`accessibility.md`, `layout.md`, `typography.md`, `color.md`,
`designing-for-macos.md`) plus 3–6 pages for what is on screen via
`references/hig-lookup.md`, read the file before citing, and cite
`<page>.md › <Heading>`. The public `developer.apple.com/…/<page>.md` endpoint
is the fallback when the local copy is missing; the `.html` pages are JS shells
and quote nothing. Never cite from memory.

## Step 1 — design review (before code)

The implementer posts on the issue, next to the architecture review:

1. **The HIG pages that govern the surface** and the sentences quoted verbatim
   from them (`<page>.md › <Heading>`). Typical pages for Kagi: `toolbars`,
   `sidebars`, `buttons`, `sheets`, `alerts`, `menus`, `the-menu-bar`, `layout`,
   `feedback`, `writing`, `accessibility`, `color`, `typography`,
   `focus-and-selection`.
2. **A figure** — SVG rendered to PNG, OpenPencil export, or a real-window
   screenshot with the new control drawn in — showing *current* and *proposed*
   at the same scale, saved to a file and opened for the owner. Words alone are
   not a proposal; the owner cannot evaluate a placement from prose.
3. **The placement rules the proposal satisfies** (checklist below) and the one
   or two it stretches, with the HIG sentence that permits the stretch. If none
   does, the proposal is wrong; change it.

An independent reviewer (Opus, exact figure + quoted HIG) answers P1/P2 before
implementation starts. The owner picks between figures when a trade-off remains.

## Step 2 — merge review (before merge)

The implementer attaches EN+JA, light+dark real-window screenshots (Tier B).
An independent reviewer reads the diff, the screenshots and the HIG pages and
posts findings on the PR in Japanese: `[P1]` contradicts quoted HIG text,
`[P2]` HIG recommends otherwise. Every P1/P2 is fixed and re-reviewed on the
exact head before merge; the PR body links the review. When the GitHub review
bot is unavailable, this review is still required — it is not a substitute for
Codex, Codex is not a substitute for it.

## Placement checklist

Each rule is tagged by where its authority comes from. A reviewer reports a
`[HIG]` rule as P1/P2 only with the quoted sentence; a `[house]` rule is a Kagi
convention and is reported as such, never as "HIG says". An `(owner ruling,
#N)` overrides both and is not re-litigated in review.

**Toolbars** `[HIG]` — `toolbars.md › Best practices`: "Choose items
deliberately to avoid overcrowding." / "Keep actions with text labels
separate." / "Make every toolbar item available as a command in the menu bar."
`[house]` Navigation (view switching) sits apart from commands; nothing in a
toolbar changes meaning with pane focus. `(owner ruling, #1764)` Conflict
session controls (中止 / スキップ / 確認へ進む) are not in the main toolbar.

**Sidebars** `[HIG]` — `sidebars.md`: "A sidebar appears on the leading side of
a view and lets people navigate between areas of your app or top-level
collections of content" / "Consider letting people hide the sidebar." The
sidebar therefore can't be the only place for a command. `(owner ruling,
#1764)` The conflict sidebar's session header carries `● <op> n/N`, 中止 and
スキップ; when the sidebar is hidden the #704 operation strip carries them, and
the Repository menu always does.

**Sidebar toggle** `[HIG]` — `toolbars.md › Item groupings`: "Elements that
let people return to the previous document and show or hide a sidebar appear at
the far leading edge". Kagi has one sidebar slot owned by the shell and one
toggle for it, in the titlebar right after the window controls, in every mode;
it is disabled, never removed, where there is no sidebar, so the tabs after it
never move. A mode does not mount its own sidebar (#1790).

**Primary / secondary / destructive** `[HIG]` — `buttons.md › Style`: "Keep the
number of prominent buttons to one or two per view." (a ceiling: zero is valid
when no action is ready). `buttons.md › Role`: "Don't assign the primary role to
a button that performs a destructive action." `alerts.md › Buttons`: "Cancel
buttons are typically on the leading side of a row". `[house]` The confirmation
names the consequence, not the command (#1588: no `git …` strings in copy).

**Status and its controls** `[HIG]` — `feedback.md › Best practices`: "Consider
integrating status feedback into your interface." `[house]` An in-progress
operation is a status label, not a button; its exit (中止) is visually separate
from its advance. `(owner ruling, #1764)` 確認へ進む sits at the trailing edge
of the file header, separated from the file-level 保存 by a divider.

**Sheets and alerts** `[HIG]` — `alerts.md › Buttons`: "you don't want to make
a Cancel button the default button… avoid making any button the default
button"; "Use a caution symbol sparingly… when confirming an action that might
result in unexpected loss of data." Escape cancels. **Conflict with a recorded
decision:** Kagi's danger sheets route root Return to Cancel
(`docs/decisions.md` 2026-10-12 #1292 / #1334, ADR-0046). Changing that to
"Return does nothing" (#1767) needs an owner ruling; until it is recorded, a
review reports the difference as a question, not as a P1.

**Feedback** — success is quiet (bounded toast/footer); failure is durable
(Operation Log) with a bounded preview; modals only when the user must decide.
Mixed-state close warnings name every kind of unsaved data they will discard,
including the scoped ordinary-editor file count alongside conflict decisions
(`feedback.md › Best practices`: "Warn people when they initiate a task that can
cause data loss that’s unexpected and irreversible."). Healthy background saves
show progress rather than failure/Retry instructions; error choices appear only
after failure (#1731/#1760 merge review).
Failure Retry retries conflict persistence only; mixed ordinary file writes remain
behind the explicitly labeled Save All action and its scoped file count.

**Layout** — controls align to the grid of their siblings (shared height,
radius, spacing tokens); a row has one reading direction; nothing is placed
where it will be covered by an animation or a popover.

**Writing / Accessibility** — labels are verbs for actions and nouns for
states, EN and JA; every control has a role, a name, a visible focus ring and
a keyboard path; status badges are labels, not disabled buttons.
Input feedback `[HIG]` — `writing.md › Best practices`: "Show errors right
next to the field, and instruct people how to enter the information correctly,
rather than scolding them for not following the rules."
`[house]` #1774: create-branch and create-worktree names stay quiet while empty
and pristine; editing or attempted Return reveals the current keyed reason,
with empty-name Return keeping field focus. Wrap the reason to preserve its
instruction; never introduce a different validator to shorten the copy.
Field-error contrast is checked against the actual painted card, not just the
modal token. Consequence summaries must agree in the visual row, AX and Copy all.

## How a finding reads

```
[P1] 中止 がツールバーにある
HIG Toolbars: "…quoted sentence…". 中止 は session の出口であり、window の
content に対する頻用コマンドではない。提案: … (図 proposal.png 案 B)。
```

## Maintenance

A PR that changes a placement rule, adds a surface class, or learns a new HIG
sentence that decided a review adds it here and states `hig skill 更新済み` in
the PR body. Rules that were decided by the owner are marked `(owner ruling,
#N)` so they are not re-litigated.
