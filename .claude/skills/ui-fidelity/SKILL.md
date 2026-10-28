---
name: kagi-ui-fidelity
description: Gate workflow for building or fixing a Kagi screen so it matches an owner-designated canonical UI (a gallery page, kit, or figure). Use before writing code for a UI rebuild/port/redesign, when a review says a screen "doesn't look like" its reference, and when fixing such a PR. Composition is proven against the canonical first, then states, then regression tests.
---

# Kagi UI fidelity gates

Canonical source: `.claude/skills/ui-fidelity/SKILL.md` (Codex:
`.agents/skills/kagi-ui-fidelity`).

Why this exists: PR #1818 spent ~30 hours proving focus rings, hover paint and
pixel edges inside a window whose *composition* (chrome, size, grouping) had
silently departed from the canonical UI at hour 4. 53 browser tests passed and
the screen still did not look like its reference. Composition is cheap to
check and decides most of what people see, so it is checked first and nothing
else starts until it passes.

This skill orders work. It does not replace `docs/ui/modern-ui.md`,
`.claude/skills/hig-review/SKILL.md` or `.claude/skills/verify/SKILL.md`.

## G0 — Name the canonical (before code)

1. Write down the **canonical UI**: the code that renders it (commit + paths)
   and its captures. For Settings it is fixed by the owner — see
   [Settings canonical](#settings-canonical) below.
2. Read the canonical **code**, not only its pictures. Every number in the
   composition table comes from that code (`pt(..)`, metrics, which kit part
   draws which row), not from eyeballing a PNG.
3. Anything else (ADRs, older figures, the previous implementation, a design
   issue) is subordinate. Where it disagrees with the canonical, list the
   conflict and ask the owner. Do not resolve a conflict by writing an ADR
   sentence or a commit message: **text is not approval**. Only an explicit
   owner reply on the PR/issue, after seeing a figure, approves a deviation.

## G1 — Composition gate (the first thing you prove)

Write `target/ui-fidelity/<surface>/composition.md` with one row per
property, canonical value from code, actual value measured, ○/×:

| Group | Rows (at least) |
|---|---|
| Window | logical width × height **and the size policy**: fixed, or grows/shrinks with content — measure it on at least the shortest and the tallest pane (does switching panes resize the window?); what scrolls when content is taller; whether host chrome (title band, toolbar) is drawn by Kagi; where the traffic lights sit relative to the sidebar |
| Sidebar | x/y/width/height of the panel; top inset; row height and row pitch; selection fill role; item order |
| Detail | title band height and the title's text; scroll content insets; gap between sections |
| Per pane | sequence of sections; for each section, which rows sit in which group card; which kit part draws each trailing control; the **exact visible text** of every title, label and button (EN and JA) compared with the canonical's strings — a shortened or reworded label is a ×, even if it fits. A canonical gallery may hard-code English **placeholder** literals (text not taken from `Msg`); those fix the English wording only, never the other locale: a JA screen showing an English placeholder is a × |
| Repeated parts | card/tile size and how many are visible at the canonical width |

Then:

1. Implement **structure only** (window, sidebar, title, sections, groups,
   which part per row). Do not start focus, hover, AX, IME, pixel-proof or
   matrix work.
2. Capture the **whole real window** — native, including anything Kagi draws
   as chrome — at the canonical's theme, language and zoom. A harness that
   does not draw the host's chrome (e.g. the browser host for Settings)
   cannot pass G1.
3. Put actual and canonical side by side at the **same logical scale**
   (divide each PNG by its own backing scale) in
   `target/ui-fidelity/<surface>/sheet-<pane>.png`, and open it.
4. Fill the "actual" column from measured bounds (renderer probes or
   `render_to_image` pixels), not from constants in your own code.
5. **Pass** = every row ○ (geometry within ±1 pt, structure exact). Any × is
   fixed, or becomes an owner question with the sheet attached. Do not
   continue past a × while waiting.

Time box: reach a G1 verdict within the first 3 hours of the task. If you
cannot, stop and report the blocking rows with the sheet. Do not fill the
time with later gates.

## G2 — Parts

For each control kind on the screen, compare it with the canonical kit part
(height, radius, fill role, chevron/thumb/selection placement, Light and
Dark). Same role, same look. Fix in the shared component, never per page.

## G3 — States and matrices

Only now: focus/hover/pressed/disabled, keyboard path, AX names, EN/JA,
100%/167%, narrow widths — using the existing recipes in the verify skill.
A state never changes data (focus is not selection is not ON).

## G4 — Regression

Turn every G1 row into an assertion on the **real owner's measured bounds**
(the native window for chrome/size; the shared view for its internals). An
assertion that compares a value with the constant that produced it proves
nothing; assert relations the canonical defines (sidebar top equals window
top inset, rows share one pitch, cards lie inside their group, N cards visible
at width W).

## Rules that apply throughout

- **Shared owner.** A visual fix goes into the shared component both native
  and browser hosts render. If the defect is in host-only code (window
  chrome), say so and verify it natively.
- **No silent re-scope.** Keeping a legacy size/chrome "to preserve existing
  behavior" is a deviation like any other: G0 rule 3.
- **Evidence budget.** Do not re-run unchanged suites to re-prove moved code.
  Run the scenarios that cover what changed, once, and cite them.
- **Report** with: the composition table, sheet paths, deviations with the
  owner reply that approved each (link), what was not observed.

## Settings canonical

Owner ruling (PR #1818 review, 2026-10-29): the Settings UI built in the
#1814 experiment is canonical.

- Code: `origin/experiment/kagi-settings-skill` @ `1c6486202`
  - parts: `src/ui/settings_kit/mod.rs`, `src/ui/settings_kit/controls.rs`
  - Kagi's eight panes: `experiments/settings-skill/runs/i10/lab_i10_s8.rs.txt`
  - theme chooser: `experiments/settings-skill/runs/i10/lab_i10_h.rs.txt`
  - read with `git show <commit>:<path>`; regenerate captures by copying a
    `.rs.txt` to `examples/` on that commit and running it per
    `examples/settings_gallery.rs` (`KAGI_GALLERY_*`).
- Captures: attached to the PR #1818 review; local copies may exist under
  `target/ui-fidelity/canon/settings/`.

Composition values taken from that code (pt = design points at 100%):

| Property | Canonical |
|---|---|
| Window | 900 × 760, **fixed**: switching panes never resizes the window, and content taller than the detail scrolls inside it. Bounded by the visible screen. No Kagi-drawn title band: the window is the sidebar + detail. |
| Traffic lights | inside the sidebar's top inset |
| Sidebar panel | `sidebar()`: margin 8, top padding 40, horizontal padding 8, gap 2, width 212, radius 12, 1 px hairline border, `ink::sidebar()` fill; full window height minus margins |
| Sidebar row | `sidebar_item()`: min height 32, padding 8 × 4, 20 pt tile, label gap 9; selected fill `ink::selected()` (= `segment_selected_bg`, neutral), semibold label |
| Sidebar tiles | category `Hue` per pane: Appearance Indigo, Graph Orange, Editor Teal, Smart Commit Purple, Terminal Gray, Network & Sync Blue, General Gray, Analyze Teal |
| Detail title | `detail_page()`: band 52 high, padding x 14, 15 pt bold, text = pane name, shown once |
| Detail scroll | padding left 6, right 18, bottom 20; 20 between sections |
| Section | heading then body, gap 8; heading inset = group padding (12) + 1 px in Light |
| Theme chooser | per appearance (Light/Dark) one `section`, whose cards sit **inside** one `group()` card; rows that belong to that chooser follow inside the same group |
| Pop-up | `popup_value()`: value text + 18 pt filled chevron box; the trigger itself is not filled at rest |
| Visible text | Where the canonical uses a `Msg`, show that `Msg` in every locale (e.g. Custom themes EN "Open theme folder…", "Reload Themes", "Theme file guide…"); never shorten an existing string to make a layout fit. Where the canonical hard-codes an English literal (s8 pane/section titles such as "Graph", "Layout", "Motion"; the s8 task allowed English placeholders), it is a placeholder: keep that English wording in EN and use the production JA translation (an existing `Msg`, e.g. owner-approved スマートコミット) or add one — never English in JA |
| Owner exceptions | no Search field (#1816: no fake controls); pane set/order per #1816 |
