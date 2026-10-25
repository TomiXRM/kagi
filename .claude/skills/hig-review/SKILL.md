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

## Placement checklist (quote the page you apply)

**Toolbars** — window-level, frequently used commands that act on the window's
content. No session exits (abort/cancel), no destructive commands, no controls
whose target is a sidebar row. Navigation (view switching) sits apart from
commands. Nothing in a toolbar changes meaning depending on which pane has
focus.

**Sidebars** — navigation and selection. A sidebar may carry a short status
header for the list it shows; it does not carry commands that act on the whole
window or end a session. Row actions are in context menus or in the row.

**Primary / secondary / destructive** — one primary action per surface, on the
trailing edge of its row or sheet; prominent (filled) buttons stay at one or
two per view (`buttons.md › Style`). A destructive action is visually
distinct, never adjacent to the primary, and an uncommon, irreversible one is
confirmed in a sheet (`alerts.md`). The confirmation names the consequence,
not the command (#1588: no `git …` strings in copy).

**Status is not a button** — an in-progress operation (merge 1/3, rebase step)
is a status banner or label; its exits (abort) and its advance (continue) are
controls next to it, separated by space, not mixed into unrelated rows.

**Sheets and alerts** `[HIG]` — `alerts.md › Buttons`: "you don't want to make
a Cancel button the default button… avoid making any button the default
button"; "Use a caution symbol sparingly… when confirming an action that might
result in unexpected loss of data." `(owner ruling, #1767)` Return does
nothing on danger sheets; confirm by click or Space/Return on the focused
button; Escape / ⌘. cancel. This supersedes the root-Return-cancels policy in
the 2026-10-12 #1292 / #1334 decisions and ADR-0046.
A caution symbol only for **unexpected** data loss, not reassuring or
progress notes or tasks whose sole purpose is deliberate overwrite/removal
(`alerts.md › Desktop (macOS)`). Classify typed note kinds, never their translated
text. The user-selected Abort confirmation retains its destructive appearance
as an explicit safety convention (owner ruling, #1767); this does not give it
default focus or a root Return action.

**Feedback** — success is quiet (bounded toast/footer); failure is durable
(Operation Log) with a bounded preview; modals only when the user must decide.
Repository notifications occupy the center content's top-trailing safe area,
below its header/help/banner and outside sidebar/Inspector, with a 45%-column
width cap (#1768). Persistent cards keep × / Escape dismissal. Home/Settings
open an in-flow lane only for Error/action-bearing cards; transient feedback
must not move controls. Recovery captions reuse the existing command name,
and a failed or unfinished refresh must not dismiss the failure preview.
Any accepted successful read for the captured owner resolves that preview,
including keyboard/menu refresh, tab reactivation and external reload; the
card's own Refresh button is not a prerequisite. Takeover centers follow the
same persistent-only lane admission, with no empty band for transient stacks.
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
