# PDCA log — Kagi settings skill experiment

Branch `experiment/kagi-settings-skill`, started 2026-10-27. Protocol:
[`../protocol.md`](../protocol.md); fixed scenarios and checklists:
[`../scenarios.md`](../scenarios.md); failure-pattern ledger:
[`../ledger.md`](../ledger.md).

Screenshots are under `shots/` next to this file. That directory is
git-ignored (AGENTS.md: screenshots are never committed), so the images render
only in the checkout where the experiment ran. Every image named here is a real
native capture unless it says "figure".

## Summary table

| Iteration | Skill | S1 | S2 | S3 | S4 | S5 | S6 | S7 | S8 | H | Change made after it |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | v0 | ○ 100 | ○ 93 | ○ 100 | — | — | — | — | — | — | v1 structured image critique (C1/C2) |
| 2 | v1 | ○ 93 | ○ 100 | ○ 100 | — | — | — | — | — | — | v2 rules at point of use |
| 3 | v2 | ○ 100 | ○ 100 | ○ 100 | — | — | — | — | — | — | v3 interaction states never change data |
| 4 | v3 | ○ 86 | ○ 100 | ○ 100 | — | — | — | — | — | — | v4 closed vocabulary, evidence completeness; + Apple S4/S5 |
| 5 | v4 | ○ 100 | ○ 93 | ○ 100 | ○ 100 | ○ 100 | — | — | — | — | F1 kit Slider/checkbox/progress; v5 category hues, page-local rules; + S6/S7 |
| 6 | v5 | ○ 100 | ○ 93 | ○ 100 | ○ 93 | ○ 100 | ○ 100 | ○ 93 | — | — | F2 offscreen Metal capture; v6 placement §3; + S8 |
| 7 | v6 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 93 | — | v7 object-named panes, no grab-bag |
| 8 | v7 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | — | F4 one CONTROL_HEIGHT; v8 same row, same height (owner) |
| 9 | v8 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | — | F5 header inset; v9 label wording |
| 10 | v9 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 100 | ○ 92 | stop (iteration 10); hold-out H |

○ = every `[critical]` item met; the number is accuracy %. — = scenario not yet part of the set.

### What the ten iterations showed

- **Accuracy saturates early; the signal is in the gap between the two
  sides.** Children reported "no unclear points" in most runs while Astra's
  captures kept finding defects (a slider thumb off its track, invented state
  data, recoloured tiles, collapsed headings). The skill improved by turning
  each evaluator finding into a rule *at the point where the decision is
  made* (ledger L1–L12), not by adding prose at the end.
- **Rules lose to reference images unless they sit next to the part.** The
  tile rule was ignored in §3 (v1), followed once moved into the kit table
  (v2), misread as "the selected tile" (v3), and stable only once it named its
  subject (v4).
- **Recurring page-local gaps belong in the kit.** Five S2 runs produced five
  different hand-made sliders, two broken; after the kit gained `Slider`
  (F1) the defect class disappeared. The same held for row heights (F4): a
  kit that mixed 22/24/26 pt taught every page to mix them.
- **Evidence must not depend on the user's screen.** Captures failed whenever
  a full-screen app covered the displays; the occlusion guard prevented stale
  evidence, and offscreen Metal rendering (F2) removed the dependency.
- **Placement is teachable.** With §3 (v6–v7) children split General into
  object-named panes and stopped inventing grab-bag panes.
- **Hold-out:** 92 % against a 100 % plateau (inside the 15-point band). Its
  invented previews were weaker than the data-driven S6 cards — the next
  rule to write is how to fake a preview when no data exists.
- Convergence by the strict rule was not reached (new, mostly environmental
  unclear points kept appearing); the run stopped at the planned ten
  iterations.

---

## P0 — References and baseline

### Plan

Fix what "good" means before writing anything: the Rex settings window the
owner chose, Apple HIG pages that govern a macOS settings window, and today's
Kagi Settings window captured the same way.

### Do

- Rex Beta Settings: the owner's three captures (Notifications, Interface,
  Themes) plus one window capture taken here (Server) once the window was on
  the visible DELL display. Clicks posted to Rex's background window did not
  change its pane (nine captures, one identical hash), so only Server was
  captured live.
- Apple HIG: `settings`, `sidebars`, `toggles`, `segmented-controls`,
  `pop-up-buttons`, `disclosure-controls`, `layout`, `designing-for-macos`,
  `color`, `typography` fetched as Markdown from developer.apple.com
  (`~/.claude/skills/apple-design` is not installed on this machine).
- Kagi baseline: plain debug build copied to
  `target/ui-lab/bin/kagi-baseline`, launched with a private `KAGI_LOG_DIR`,
  `KAGI_NO_ACTIVATE=1`, a fresh repository, and Settings opened with ⌘, posted
  to its PID.

### Check

| Rex Notifications | Rex Interface | Rex Themes | Rex Server (live) |
|---|---|---|---|
| ![](shots/reference/rex-notifications.png) | ![](shots/reference/rex-interface.png) | ![](shots/reference/rex-themes.png) | ![](shots/reference/rex-server.png) |

Kagi today (Apple Dark, EN, 1×):

![Kagi Settings baseline](shots/baseline/kagi-settings-general-en-dark.png)

What Rex does that Kagi does not, measured on these captures:

- Navigation is a leading inset sidebar of icon tiles; Kagi uses a centred
  tab row.
- Settings sit in inset rounded cards whose rows share one height and one
  trailing edge; Kagi's rows put the control in a 48 % column with its top at
  the title baseline, and descriptions run under both.
- Section titles are bold body text; Kagi's are small grey caps with a rule.
- Two-option choices are segmented controls or switches; Kagi uses radio
  pairs.
- No help paragraph is visible by default; Kagi prints long explanations
  inline (Compact layout threshold, Avatar commit nodes).

T3 Code (owner's second reference: "few plain buttons; the text field is not
a box") could not be captured: its window was on a full-screen Space of the
built-in display, where `screencapture -l` cannot read it. Its principle is
carried into the skill as text only — controls are capsules or ghost actions,
inputs are filled fields without heavy borders.

### Act

Scenarios S1–S3 and hold-out H frozen in `scenarios.md` from these references.

---

## P1 — Foundation: kit, gallery host, design review

### Plan

Hypothesis: issues #1799/#1800/#1802 say the missing piece is one owner per
control plus a place to see parts side by side. Without that, no skill can
make pages consistent, so the foundation comes before skill tuning.

Feature architecture review (shown before code):

```text
1. SEARCH: settings row helpers are private in src/ui/settings_view.rs
   (section_header :353, settings_group :366, setting_row :381,
   setting_row_with_help :440); switch wrapper keyboard_nav::switch
   (src/ui/keyboard_nav.rs:150); chips hand-made in settings_controls.rs:187;
   kagi_text_field / kagi_search_field / kagi_segmented: 0 hits in src/ and
   crates/; no gallery (examples/modal_shot.rs opens one modal; the inventory
   runner captures KagiApp screens through gui-e2e and needs exactly one
   window).
2. OWNER: settings form geometry = settings_view.rs private helpers; screen
   captures = tests/recovery/inventory (gui-e2e).
3. DECISION: CREATE src/ui/settings_kit.rs as the single public owner of
   settings parts (the private helpers cannot be reached by examples and
   carry today's look), and CREATE src/ui/gallery_host.rs because inventory
   targets need a KagiApp fixture, the gui-e2e build and a shared registry
   that parallel implementers would conflict on. The host keeps inventory's
   naming (<name>-<lang>-<theme>.png) so targets can move into inventory at
   import.
4. REFACTOR: production settings_view.rs is NOT switched in this experiment
   (no production-wide rewrite); the parallel owner is recorded as import
   debt in ../import.md with its removal condition.
5. ANTI-PATTERNS: #1 no manager; #3 capture I/O runs in a spawned task after
   the window opened, not in render; #4 every kit part is used by the
   reference gallery and by every lab page; #7 flagged above (temporary
   parallel owner until import).
```

HIG Step 1: figure of current vs proposed at the same logical scale, reviewed
by an independent Claude Opus reviewer before code
([`../hig-review-step1.md`](../hig-review-step1.md)).

![figure: current vs proposed, same scale](shots/design/figure.png)

Verdict `PROCEED` (no P1). P2s taken into the kit and skill: equal segment
widths; whole pop-up value is one ≥ 20 pt target; mini switch; sidebar tiles
follow the accent unless the colour means something; "…" on buttons that open
another window or app; chevron rows only for in-window navigation. Owner
decision still open: sidebar (System Settings style, a recorded deviation)
or toolbar panes (HIG default).

### Do

- `src/ui/settings_kit.rs`: window scaffold, sidebar, search look, sidebar
  item, icon tile, page, section (+ accent action), group with inset
  hairlines, `Row` (leading / subtitle / trailing / navigation), mini switch,
  text-only `Segmented`, pop-up value, stepper, capsule button, divider,
  caption, ring; colours only from theme roles (`ink::*`), sizes only through
  `pt()`; `Forced` draws hover / pressed / focus / disabled.
- `src/ui/gallery_host.rs`: `KAGI_GALLERY_{PAGE,STATE,THEME,LANG,ZOOM,SIZE,
  DISPLAY,OUT,EXIT_AFTER_MS}`, `--list` manifest from code, unknown page /
  state / theme / language fail with the valid names, private `KAGI_LOG_DIR`,
  no activation, unfocused window, capture refuses an occluded window.
- `examples/settings_gallery.rs`: `parts` (every part × 5 states) and `server`
  (Rex Server pane from kit parts only).

### Check

| server · Apple Dark | server · Apple Light |
|---|---|
| ![](shots/foundation/server-default-apple-dark.png) | ![](shots/foundation/server-default-apple-light.png) |

| parts · rest · dark | parts · hover · dark | parts · focus · light |
|---|---|---|
| ![](shots/foundation/parts-rest-apple-dark.png) | ![](shots/foundation/parts-hover-apple-dark.png) | ![](shots/foundation/parts-focus-apple-light.png) |

- `--list` printed 8 targets; `KAGI_GALLERY_STATE=nope` exited 2 with
  `unknown state "nope" for page parts; states: rest, hover, pressed, focus,
  disabled`.
- Not observed: real pointer hover, keyboard focus traversal and VoiceOver —
  background windows never receive them (#1799); forced states are drawn.

### Act

Skill v0 written (`../skill/SKILL.md`). Iteration 0 (static description/body
check) found one mismatch: the description promised "change a Kagi settings
page", but the body only covers gallery pages because production Settings
does not use the kit yet. The description was narrowed before iteration 1.

---

## Iteration 1

### Plan

Baseline with skill v0. Hypothesis: kit table + one worked example + capture command are enough for a fresh implementer to reach critical success on S1–S3 without touching `src/`.

### Do

Skill snapshot: [`runs/i01/skill.md`](../runs/i01/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i01/`](../runs/i01/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 630 s | 55 | 3 (return type, viewport height, JA sidebar wording) |
| S2 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 296 s | 76 | 3 (two API fixes, one recapture) |
| S3 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 406 s | 41 | 0 |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is recorded as a proxy and is not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 93% | 1=○; 2=○; 3=○; 4=○; 5=△; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |

**S1** — `default-apple-dark-en.png`, `position-rest-apple-dark-en.png`, `position-selected-apple-dark-en.png`, `position-focus-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | position-rest-apple-dark-en | position-selected-apple-dark-en |
|---|---|---|
| ![](shots/i01/s1/default-apple-dark-en.png) | ![](shots/i01/s1/position-rest-apple-dark-en.png) | ![](shots/i01/s1/position-selected-apple-dark-en.png) |

- S1: Evaluator: state `position-rest` draws no selected cell although the page value is bottom-right (state semantics invented by implementer).

**S2** — `default-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-choice-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i01/s2/default-apple-dark-en.png) | ![](shots/i01/s2/focus-choice-apple-dark-en.png) | ![](shots/i01/s2/default-apple-light-en.png) |

- S2 item 5 slider + pop-ups: partial — slider thumb drawn after the track end, detached from the fill at value 3/5 (self-check said ○)
- S2: Evaluator: sidebar tiles use five fixed colours (Success/Tag/Danger/Remote) despite skill §3 "Tint::Accent unless colour carries meaning" — rule not applied (C2 sibling mismatch with S1/S3).
- S2: Evaluator: preview tiles are filled with the accent colour, so all five previews look alike; the selected ring is the only differentiator.

**S3** — `default-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i01/s3/default-apple-light-ja.png) | ![](shots/i01/s3/default-apple-light-ja-720x692.png) | ![](shots/i01/s3/default-apple-dark-en.png) |

- S3: Evaluator: JA wrap puts "。" at a line start (GPUI line breaking, not the page) — recorded as a kit/engine observation, not scored.

All three children reported "Unclear points: none" and scored themselves ○ on every item. Astra's capture of the same sources agreed on the critical items (all renders exit 0, `--list` matches, unknown state exits 2, no hex colours or raw `px()` in any page) but found defects the self-checks passed over — the signal of this iteration is in the gap between the two sides, not in the accuracy numbers.

#### Unclear points (self-reported + evaluator)

- (self) none in S1, S2, S3.
- (evaluator) **[Execution] S2** Issue: slider thumb drawn beyond the end of its track while the fill stops at 3/5; self-check ○. Cause: skill §4 said "open every PNG and check it against the reference" with no criteria. General Fix Rule → L1.
- (evaluator) **[Planning] S2** Issue: sidebar tiles copied in Rex's five fixed colours although §3 says accent unless the colour means something. Cause: §3 comes after the code and the table; the reference image was consulted first. General Fix Rule → L2.
- (evaluator) **[Understanding] S1** Issue: state `position-rest` removes the selected cell, i.e. changes data to differ from `default`. Cause: "every state must look different" in the example comment. General Fix Rule → L3.

### Act

Theme for v1: **structured image critique** (#1798's C1/C2). The old one-line "check it against the reference" became §5 with five yes/no criteria and `[C1]`/`[C2]` findings in the report; "§3 beats the reference" is one of the criteria (L1, L2 share the cause "unstructured check", so they are one theme). L3 is a different theme and waits for v2.

Expected effect, tied to checklist text: S2 item 5 ("slider row shows tick marks and a trailing value", scored partial on a detached thumb) and the evaluator issues L1/L2; no checklist item targets tile colour, so L2's effect is visible only as an evaluator issue.

---

## Iteration 2

### Plan

Skill v1 (structured image critique, §5). Hypothesis: with five named criteria the implementer finds geometry defects (L1) and reference-vs-rule conflicts (L2) before reporting. Models swapped: Terra on S1/S3, Sol on S2.

### Do

Skill snapshot: [`runs/i02/skill.md`](../runs/i02/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i02/`](../runs/i02/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 197 s | 73 | 2 (collapsed picker cells, missing picker children) |
| S2 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 810 s | 75 | 7 implementation rounds + 1 patch + 1 capture |
| S3 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 334 s | 66 | 1 (unknown-state check run with --list) |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is recorded as a proxy and is not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 93% | 1=○; 2=○; 3=△; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |

**S1** — `default-apple-dark-en.png`, `position-selected-apple-dark-en.png`, `position-focus-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | position-selected-apple-dark-en | position-focus-apple-dark-en |
|---|---|---|
| ![](shots/i02/s1/default-apple-dark-en.png) | ![](shots/i02/s1/position-selected-apple-dark-en.png) | ![](shots/i02/s1/position-focus-apple-dark-en.png) |

- S1 item 3 structure: partial — code has all four blocks, but the 900×692 page cuts off the last group: no capture shows "Show each process in tabs" off
- S1: Evaluator: sidebar tiles mix Accent, Success, Danger and Neutral without status meaning ("normalized to Kagi's accent/status scheme") — L2 recurs after v1.

**S2** — `default-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-choice-apple-dark-en | focus-slider-apple-dark-en |
|---|---|---|
| ![](shots/i02/s2/default-apple-dark-en.png) | ![](shots/i02/s2/focus-choice-apple-dark-en.png) | ![](shots/i02/s2/focus-slider-apple-dark-en.png) |

- S2: Sidebar tiles all Accent (L2 rule followed by this child).

**S3** — `default-apple-dark-en.png`, `focus-ui-zoom-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i02/s3/default-apple-light-ja.png) | ![](shots/i02/s3/default-apple-light-ja-720x692.png) | ![](shots/i02/s3/default-apple-dark-en.png) |


S2's slider is now correct and the child reported and fixed a `[C1]` (light/dark preview colours inverted) — L1 worked where it was applied. S2 and S3 followed the tile rule. S1 (Terra) wrote a correct page but did not size it, so the evidence does not show the last group, and it recoloured the sidebar with status tints it justified itself.

#### Unclear points (self-reported + evaluator)

- (self) **[Understanding] S2** Issue: the general "2–5 choices → Segmented" rule overlapped the scenario's preview tiles. Cause: the rule had no scope. General Fix Rule → L5.
- (self) **[Execution] S2** Issue: inline image preview looked empty until the PNG pixels were checked. Cause: tool display, not the skill — recorded, no skill change.
- (evaluator) **[Planning] S1** Issue: non-accent sidebar tiles. Cause: the v1 fix sits in §5, which is read after the page is built. **Recurrence of L2.**
- (evaluator) **[Execution] S1** Issue: last group below the fold in every capture. General Fix Rule → L4.

### Act

Theme for v2: **rules at the point of use** (L2 recurrence + L5 share it). Following the ledger, the existing fix was not repeated but moved: the tile and segmented rules now live in the §2 kit-table rows that the implementer reads while choosing each part, with their scope ("text choices; visual previews are a choice tile"); §3 keeps only rules no part owns, and the §5 line was renamed "the skill beats the reference". L3 (state semantics) and L4 (evidence completeness) wait.

Expected effect: evaluator issue L2 disappears in S1/S2; no checklist item measures tile colour, so the effect shows only in the evaluator column.

---

## Iteration 3

### Plan

Skill v2 (rules at the point of use). Hypothesis: with the tile and segmented rules in the kit-table rows, no child recolours sidebar tiles from the reference, and the segmented/preview-tile overlap (L5) is not reported again.

### Do

Skill snapshot: [`runs/i03/skill.md`](../runs/i03/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i03/`](../runs/i03/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 294 s | 39 | 3 (icon names/return type, picker contrast + window height) |
| S2 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 538 s | 83 | 1 (shell variable in its own check) |
| S3 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 219 s | 24 | 1 (unavailable IconName::ChartBar) |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is recorded as a proxy and is not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |

**S1** — `default-apple-dark-en.png`, `rest-apple-dark-en.png`, `selected-apple-dark-en.png`, `hover-apple-dark-en.png`, `focus-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-apple-dark-en | selected-apple-dark-en |
|---|---|---|
| ![](shots/i03/s1/default-apple-dark-en.png) | ![](shots/i03/s1/rest-apple-dark-en.png) | ![](shots/i03/s1/selected-apple-dark-en.png) |

- S1: Evaluator: state `rest` draws no selected cell and swaps the subtitle to "Choose a position" — L3 again (rule still pending).
- S1: Evaluator: in Apple Light the selected picker cell is a pale grey check on a pale cell; the §5 "both themes read" line did not catch it (L1 class).

**S2** — `default-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `pressed-choice-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-slider-apple-dark-en | pressed-choice-apple-dark-en |
|---|---|---|
| ![](shots/i03/s2/default-apple-dark-en.png) | ![](shots/i03/s2/focus-slider-apple-dark-en.png) | ![](shots/i03/s2/pressed-choice-apple-dark-en.png) |

- S2: Evaluator: non-selected sidebar tiles neutral, selected accent — v2 row "its tile is always Tint::Accent" read as the selected tile only (L7).
- S2: Evaluator: Interface-style cluster spreads Glass and Classic to opposite edges while Color scheme is packed — a sibling-layout mismatch the critique did not flag.

**S3** — `default-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i03/s3/default-apple-light-ja.png) | ![](shots/i03/s3/default-apple-light-ja-720x692.png) | ![](shots/i03/s3/default-apple-dark-en.png) |


All checklist items ○ in all three scenarios — accuracy is saturated, so the remaining signal is the evaluator column. L5 did not recur; L4 did not recur (S1 sized its window to 900×760). The tile rule now reached every child, but Terra read it as applying to the selected row only. Two children paid a compile round for icon names they guessed.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S1, S3** Issue: initial icon names were unavailable. Cause: the skill names `IconName` but not its variants. General Fix Rule → L6.
- (evaluator) **[Understanding] S1** Issue: `rest` drops the selected cell (L3, second occurrence; no fix yet).
- (evaluator) **[Understanding] S2** Issue: only the selected sidebar tile is accent. Cause: v2 wrote "its tile". General Fix Rule → L7.
- (evaluator) **[Execution] S1, S2** Issue: low-contrast light picker; unevenly spread cluster. Cause: §5 criteria exist but were answered "none" — the critique is self-graded. Recorded under L1; not yet a skill change.

### Act

Theme for v3: **state semantics** (L3 — the only issue seen twice and still without a fix). §1's example comment no longer says "every state must look different"; §2 adds "Interaction states never change data" and a separate naming rule for value states, so checklist S1 item 4 ("rest, selected, focus" states) stays satisfiable without changing data under an interaction name. L6 and L7 wait for v4.

---

## Iteration 4

### Plan

Skill v3 (interaction states never change data). Hypothesis: no state drops or changes the page's values.

### Do

Skill snapshot: [`runs/i04/skill.md`](../runs/i04/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i04/`](../runs/i04/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 340 s | 64 | 1 (picker ids) + refused captures |
| S2 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 644 s | 80 | 4 builds + 3 refused captures + 2 visual rounds |
| S3 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 167 s | 87 | 1 (missing import) |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 86% | 1=○; 2=○; 3=△; 4=△; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |

**S1** — `default-apple-dark-en.png`, `rest-apple-dark-en.png`, `selected-apple-dark-en.png`, `focus-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `hover-switch-apple-dark-en.png`, `focus-nav-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-apple-dark-en | selected-apple-dark-en |
|---|---|---|
| ![](shots/i04/s1/default-apple-dark-en.png) | ![](shots/i04/s1/rest-apple-dark-en.png) | ![](shots/i04/s1/selected-apple-dark-en.png) |

- S1 item 3 structure: partial — 900×692 page again cuts off the last group in every capture
- S1 item 4 picker reusable: partial — unselected cells are drawn in the card colour and vanish; only the selected cell is visible
- S1: Evaluator: the child never saw a light/dark capture (host refused: display occluded) yet self-scored the visual items ○.
- S1: Evaluator: only the selected sidebar tile is accent (L7 recurs).

**S2** — `default-apple-dark-en.png`, `hover-choice-apple-dark-en.png`, `pressed-choice-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `disabled-choice-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-choice-apple-dark-en | pressed-choice-apple-dark-en |
|---|---|---|
| ![](shots/i04/s2/default-apple-dark-en.png) | ![](shots/i04/s2/hover-choice-apple-dark-en.png) | ![](shots/i04/s2/pressed-choice-apple-dark-en.png) |

- S2: All sidebar tiles accent; seven interaction states, data unchanged.

**S3** — `default-apple-dark-en.png`, `focus-auto-fetch-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i04/s3/default-apple-light-ja.png) | ![](shots/i04/s3/default-apple-light-ja-720x692.png) | ![](shots/i04/s3/default-apple-dark-en.png) |

- S3: Evaluator: non-selected sidebar tiles neutral (L7 recurs).

L3 held in all three scenarios (S1 now keeps the selection in `rest` and adds a separate `selected` state that updates its subtitle). During this iteration the DELL display was partly covered: the host refused captures of occluded windows (`occlusionState 8192`) as designed (#1799), and S1/S2 retried. S1 then self-scored visual items it never saw. The tile wording of v2 is now misread by both Terra runs.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S1, S2** Issue: capture refused, window not visible. Cause: environment (display covered). The guard behaved correctly; the skill did not say what to do then. General Fix Rule → L4 (extended).
- (self) **[Execution] S2** unavailable icon names (L6, second occurrence).
- (evaluator) **[Understanding] S1, S3** only the selected sidebar tile accent (L7, recurring).
- (evaluator) **[Execution] S1** last group cut off again (L4, recurring); unselected picker cells invisible (L1 class).

### Act

Theme for v4: **closed, unambiguous inputs** — batched because each item recurred from more than one source (the method's exception for convergent evidence): the tile rule names its subject ("every sidebar tile — selected or not"); the full `IconName` vocabulary is listed; and §4 says a capture is evidence only for what it shows (size the page so nothing is cut; an uncaptured item is partial, never ○).

From iteration 5 the owner's Apple System Settings captures (Sound, iCloud) join as S4/S5 (`scenarios.md` amendment), because S1–S3 have saturated.

---

## Iteration 5

### Plan

Skill v4 (closed, unambiguous inputs: tile rule names its subject, full IconName list, "a capture is evidence only for what it shows"). Apple Sound (S4) and iCloud (S5) join.

### Do

Skill snapshot: [`runs/i05/skill.md`](../runs/i05/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i05/`](../runs/i05/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 329 s | 40 | 2 refused captures + 1 hover recapture + 1 visual fix |
| S2 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 447 s | 86 | 4 (2 API, 1 refused capture, 1 visual) |
| S3 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 230 s | 40 | 2 refused captures |
| S4 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 425 s | 89 | 2 (API, 1 refused capture) |
| S5 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 445 s | 51 | 3 layout/capture rounds |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 93% | 1=○; 2=○; 3=○; 4=○; 5=△; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |

**S1** — `default-apple-dark-en.png`, `rest-picker-apple-dark-en.png`, `selected-top-left-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `pressed-picker-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `hover-nav-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-picker-apple-dark-en | selected-top-left-apple-dark-en |
|---|---|---|
| ![](shots/i05/s1/default-apple-dark-en.png) | ![](shots/i05/s1/rest-picker-apple-dark-en.png) | ![](shots/i05/s1/selected-top-left-apple-dark-en.png) |

- S1: All sidebar tiles accent (L7 held).

**S2** — `default-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-choice-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i05/s2/default-apple-dark-en.png) | ![](shots/i05/s2/focus-choice-apple-dark-en.png) | ![](shots/i05/s2/default-apple-light-en.png) |

- S2 item 5 slider + pop-ups: partial — tick dots unevenly spaced and clustered left of the thumb; the track is a filled box, not a track
- S2: All sidebar tiles accent (L7 held). Third different hand-made slider in five S2 runs.

**S3** — `default-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i05/s3/default-apple-light-ja.png) | ![](shots/i05/s3/default-apple-light-ja-720x692.png) | ![](shots/i05/s3/default-apple-dark-en.png) |


**S4** — `default-apple-dark-en.png`, `focus-alert-volume-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-alert-volume-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i05/s4/default-apple-dark-en.png) | ![](shots/i05/s4/focus-alert-volume-apple-dark-en.png) | ![](shots/i05/s4/default-apple-light-en.png) |

- S4: Evaluator: sidebar lists invented categories rather than Apple's; not scored.

**S5** — `default-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `hover-manage-apple-dark-en.png`, `focus-nav-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-card-apple-dark-en | hover-manage-apple-dark-en |
|---|---|---|
| ![](shots/i05/s5/default-apple-dark-en.png) | ![](shots/i05/s5/focus-card-apple-dark-en.png) | ![](shots/i05/s5/hover-manage-apple-dark-en.png) |

- S5: Evaluator: cards are loose on the page instead of inside a group card as in the reference; not a checklist item.

L4, L6 and L7 held in every run (all sidebar tiles accent, every page sized to show its content, no guessed icon names). The new Apple scenarios passed on the first attempt with honest kit-gap lists. The one defect is again a hand-made slider (S2, Terra): five S2 runs produced five different sliders, two of them wrong. Captures were refused several times because the external display was partly covered — the host's occlusion guard prevented stale evidence.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S1, S3, S4** Issue: "not visible" capture refusals. Cause: environment (display covered). Handled by the v4 rule (retry; uncaptured = partial).
- (self) **[Planning] S1** last row below a 720×692 viewport — handled by sizing (L4).
- (self) **[Execution] S4** no Apple HIG corpus installed — environment, recorded.
- (self) **[Execution] S5** a page-local wrapper collapsed headings until flex sizing was fixed. General Fix Rule → L8.
- (evaluator) **[Execution] S2** slider ticks unevenly spaced (L1 class; third recurrence for page-local indicator geometry).

### Act

Two separate changes, recorded separately because the first is a foundation change that confounds the skill effect:

- **Foundation F1** (kit): `Slider` (one value→x mapping for fill, thumb and ticks; symbol or caption ends), `checkbox`, `progress_bar` added to `settings_kit` — the three controls that appeared as page-local gaps in ≥2 scenarios. The kit split into `settings_kit/{mod,controls}.rs` to stay under the 800-line ceiling. The `parts` gallery shows them in every forced state.
- **Skill v5**: the kit table lists the new controls; the page-local rule gains "derive every indicator position from one value→position function" and "a wrapper around kit parts keeps `w_full()` / `min_w_0()`" (L8).

S6 (Kagi theme cards, real theme data) and S7 (theme editor sheet) join from iteration 6 at the owner's request.

---

## Iteration 6

### Plan

Skill v5 + foundation F1 (kit Slider / checkbox / progress_bar; category hues for tiles at the owner's request). Seven scenarios: S6 (Kagi theme cards from real theme data) and S7 (theme editor sheet) join. The first dispatch pointed children at a display that a full-screen app covered; it was cancelled and redispatched (`dispatch.json` note).

### Do

Skill snapshot: [`runs/i06/skill.md`](../runs/i06/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i06/`](../runs/i06/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 512 s | 83 | 4 (1 + 3 refused captures) |
| S2 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 317 s | 50 | 1 build + 3 refused captures |
| S3 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 123 s | 57 | 1 (shell variable) |
| S4 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 443 s | 43 | 2 refused capture configs |
| S5 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 508 s | 95 | 4 refused captures |
| S6 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 335 s | 36 | 1 build + 4 refused captures |
| S7 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 504 s | 105 | 4 builds + 3 refused captures |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 93% | 1=○; 2=○; 3=○; 4=○; 5=△; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 93% | 1=○; 2=○; 3=○; 4=○; 5=△; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S6 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S7 | ○ | 93% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=△; 7=○ |

**S1** — `default-apple-dark-en.png`, `rest-picker-apple-dark-en.png`, `selected-picker-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `hover-switch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-picker-apple-dark-en | selected-picker-apple-dark-en |
|---|---|---|
| ![](shots/i06/s1/default-apple-dark-en.png) | ![](shots/i06/s1/rest-picker-apple-dark-en.png) | ![](shots/i06/s1/selected-picker-apple-dark-en.png) |

- S1: Sidebar tiles in category hues (v5 held).

**S2** — `default-apple-dark-en.png`, `hover-choice-apple-dark-en.png`, `pressed-choice-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `disabled-choice-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-choice-apple-dark-en | pressed-choice-apple-dark-en |
|---|---|---|
| ![](shots/i06/s2/default-apple-dark-en.png) | ![](shots/i06/s2/hover-choice-apple-dark-en.png) | ![](shots/i06/s2/pressed-choice-apple-dark-en.png) |

- S2 item 5 slider + pop-ups: partial — kit Slider thumb is white on the white light card and disappears (foundation defect F1, fixed as F2)

**S3** — `default-apple-dark-en.png`, `focus-auto-fetch-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i06/s3/default-apple-light-ja.png) | ![](shots/i06/s3/default-apple-light-ja-720x692.png) | ![](shots/i06/s3/default-apple-dark-en.png) |


**S4** — `default-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `hover-play-apple-dark-en.png`, `slider-min-apple-dark-en.png`, `slider-max-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-slider-apple-dark-en | hover-play-apple-dark-en |
|---|---|---|
| ![](shots/i06/s4/default-apple-dark-en.png) | ![](shots/i06/s4/focus-slider-apple-dark-en.png) | ![](shots/i06/s4/hover-play-apple-dark-en.png) |

- S4 item 5 two-choice + Mute: partial — Output/Input does not span the group (kit Segmented has intrinsic widths); reported as a planning conflict
- S4: Evaluator: the "Output & Input" heading collapses to one letter per line inside a page-local wrapper — the v5 `w_full()/min_w_0()` rule was not applied (L8 recurs).

**S5** — `default-apple-dark-en.png`, `hover-manage-apple-dark-en.png`, `focus-help-apple-dark-en.png`, `hover-help-apple-dark-en.png`, `pressed-help-apple-dark-en.png`, `disabled-help-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-manage-apple-dark-en | focus-help-apple-dark-en |
|---|---|---|
| ![](shots/i06/s5/default-apple-dark-en.png) | ![](shots/i06/s5/hover-manage-apple-dark-en.png) | ![](shots/i06/s5/focus-help-apple-dark-en.png) |


**S6** — `default-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `themes-middle-apple-dark-en.png`, `themes-end-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-card-apple-dark-en | themes-middle-apple-dark-en |
|---|---|---|
| ![](shots/i06/s6/default-apple-dark-en.png) | ![](shots/i06/s6/focus-card-apple-dark-en.png) | ![](shots/i06/s6/themes-middle-apple-dark-en.png) |

- S6: Evaluator: "Color Vision (Blue / Orange)" wraps mid-word under its card; the child also re-planned the sidebar (Git, AI, Language panes) unprompted.

**S7** — `default-apple-dark-en.png`, `focus-name-apple-dark-en.png`, `focus-swatch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-name-apple-dark-en | focus-swatch-apple-dark-en |
|---|---|---|
| ![](shots/i06/s7/default-apple-dark-en.png) | ![](shots/i06/s7/focus-name-apple-dark-en.png) | ![](shots/i06/s7/focus-swatch-apple-dark-en.png) |

- S7 item 6 sheet surface: partial — the sheet is taller than the window: its top edge and title touch the window edge
- S7: Evaluator: the "Preview" label of the Colors switch is missing.

Children could not capture this round: every display was covered, and the host refused occluded windows. They reported their visual items as "not captured" (the v4 rule held) instead of claiming ○. Astra's evaluation used the new offscreen Metal capture (foundation F2), so all 40 captures exist.

The new scenarios came out well on first contact: S6 draws every built-in theme from its own `bg_base` / `term_*` / accent fields, and S7 computes every hex from the theme. Two kit-level defects surfaced through children's use rather than the gallery: the kit Slider's white thumb vanishes on a light card, and `Segmented` cannot span a group.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S1, S2, S4, S5, S6, S7** Issue: captures refused (occluded). Cause: environment. Fixed in the foundation (F2): `--features gui-e2e` renders offscreen.
- (self) **[Planning] S1** Issue: HIG wants a proposal figure but only one file may be written. Cause: protocol vs. repository rule; recorded — the gallery capture is the figure in this experiment.
- (self) **[Planning] S4** Issue: full-width two-choice control not possible with kit `Segmented`. Cause: kit gap. → foundation backlog.
- (evaluator) **[Execution] S4** collapsed heading in a page-local wrapper (L8 recurs after v5).
- (evaluator) **[Execution] S7** sheet taller than its window; missing switch label.

### Act

Foundation **F2**: the host renders `KAGI_GALLERY_OUT` captures offscreen through Metal when built with `--features gui-e2e` (`Window::render_to_image`, the Tier A path): no window appears, Spaces and occlusion no longer matter. The kit Slider's thumb gets its own edge and shadow.

Skill **v6** (owner request, theme "placement"): a new §3 "Decide placement before composing" — one pane per domain, General only for app-wide basics, group by what a setting affects, order by frequency, two levels at most — and §5 now builds with `--features gui-e2e`. S8 (Kagi settings information architecture) joins from iteration 7.

---

## Iteration 7

### Plan

Skill v6 (placement section) + foundation F2 (offscreen Metal capture) and F3 (Slider thumb edge in light, `Segmented::full_width()`; F3 landed a few minutes into the iteration, before most children captured). S8 (Kagi information architecture) joins: eight scenarios.

### Do

Skill snapshot: [`runs/i07/skill.md`](../runs/i07/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i07/`](../runs/i07/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 387 s | 38 | — |
| S2 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 296 s | 89 | 2 (binary replaced by a non-gui-e2e build) |
| S3 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 300 s | 40 | 1 |
| S4 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 366 s | 72 | — |
| S5 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 665 s | 62 | 3 C1/C2 fixes |
| S6 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 408 s | 61 | 1 |
| S7 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 447 s | 38 | — |
| S8 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 629 s | 98 | 2 |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S6 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S7 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S8 | ○ | 93% | 1=○; 2=○; 3=△; 4=○; 5=○; 6=○; 7=○ |

**S1** — `default-apple-dark-en.png`, `rest-position-apple-dark-en.png`, `selected-top-left-apple-dark-en.png`, `selected-top-right-apple-dark-en.png`, `selected-bottom-right-apple-dark-en.png`, `focus-position-apple-dark-en.png`, `hover-position-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-position-apple-dark-en | selected-top-left-apple-dark-en |
|---|---|---|
| ![](shots/i07/s1/default-apple-dark-en.png) | ![](shots/i07/s1/rest-position-apple-dark-en.png) | ![](shots/i07/s1/selected-top-left-apple-dark-en.png) |

- S1: Category hues; picker readable in light.

**S2** — `default-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `hover-popup-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-choice-apple-dark-en | hover-popup-apple-dark-en |
|---|---|---|
| ![](shots/i07/s2/default-apple-dark-en.png) | ![](shots/i07/s2/focus-choice-apple-dark-en.png) | ![](shots/i07/s2/hover-popup-apple-dark-en.png) |


**S3** — `default-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i07/s3/default-apple-light-ja.png) | ![](shots/i07/s3/default-apple-light-ja-720x692.png) | ![](shots/i07/s3/default-apple-dark-en.png) |

- S3: Self-reported: v6 §3 asks for domain panes while S3 fixes a General pane with Graph/Network sections — the child kept S3 (L10).

**S4** — `default-apple-dark-en.png`, `focus-alert-volume-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-alert-volume-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i07/s4/default-apple-dark-en.png) | ![](shots/i07/s4/focus-alert-volume-apple-dark-en.png) | ![](shots/i07/s4/default-apple-light-en.png) |


**S5** — `default-apple-dark-en.png`, `hover-card-apple-dark-en.png`, `pressed-card-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `disabled-card-apple-dark-en.png`, `focus-manage-apple-dark-en.png`, `hover-nav-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-card-apple-dark-en | pressed-card-apple-dark-en |
|---|---|---|
| ![](shots/i07/s5/default-apple-dark-en.png) | ![](shots/i07/s5/hover-card-apple-dark-en.png) | ![](shots/i07/s5/pressed-card-apple-dark-en.png) |


**S6** — `default-apple-dark-en.png`, `hover-apple-light-apple-dark-en.png`, `hover-apple-dark-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-apple-light-apple-dark-en | hover-apple-dark-apple-dark-en |
|---|---|---|
| ![](shots/i07/s6/default-apple-dark-en.png) | ![](shots/i07/s6/hover-apple-light-apple-dark-en.png) | ![](shots/i07/s6/hover-apple-dark-apple-dark-en.png) |


**S7** — `default-apple-dark-en.png`, `focus-name-apple-dark-en.png`, `focus-swatch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-name-apple-dark-en | focus-swatch-apple-dark-en |
|---|---|---|
| ![](shots/i07/s7/default-apple-dark-en.png) | ![](shots/i07/s7/focus-name-apple-dark-en.png) | ![](shots/i07/s7/focus-swatch-apple-dark-en.png) |


**S8** — `default-apple-dark-en.png`, `pane-general-apple-dark-en.png`, `pane-graph-apple-dark-en.png`, `pane-network-sync-apple-dark-en.png`, `pane-behaviour-apple-dark-en.png`, `pane-language-apple-dark-en.png`, `pane-smart-commit-apple-dark-en.png`, `pane-analyze-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | pane-general-apple-dark-en | pane-graph-apple-dark-en |
|---|---|---|
| ![](shots/i07/s8/default-apple-dark-en.png) | ![](shots/i07/s8/pane-general-apple-dark-en.png) | ![](shots/i07/s8/pane-graph-apple-dark-en.png) |

- S8 item 3 General / domains: partial — Graph and Network moved out, but a "Behaviour" grab-bag pane remains (⌘C copies, Markdown mode, terminal lock, motion) and Language became a one-setting pane while General holds Appearance
- S8: Evaluator: the Custom themes actions sit outside their group card (C2).

First iteration in which every child could see its own captures (offscreen render). Visual quality is now consistent across eight pages and both themes; children self-reported and fixed C1/C2 findings themselves (S5). The remaining issues are about *placement*, which is what the owner asked to start measuring: S8 split Graph and Network out of General but invented a "Behaviour" grab-bag pane and a one-setting Language pane, and S3's child found that v6's placement rule contradicts S3's fixed layout.

#### Unclear points (self-reported + evaluator)

- (self) **[Planning] S3** v6 §3 vs a scenario that fixes the layout. General Fix Rule → L10.
- (self) **[Execution] S2, S3** a concurrent build without `gui-e2e` replaced the shared example binary. Cause: one child built without the feature (environment of shared checkout); v6 already says "always build with it".
- (self) **[Understanding] S4, S7** no speaker icon; no theme fields for Cursor Text / Selected Text — catalog/data gaps, recorded.
- (self) **[Execution] S6** a broad search touched another lab file (protocol breach, self-reported; no influence visible).
- (evaluator) **[Planning] S8** grab-bag "Behaviour" pane and a one-setting pane. General Fix Rule → L9.

### Act

Skill **v7** (theme: placement precision, L9 + L10): panes are named for the *object* a setting changes, with a worked list (Graph ⌘C → Graph, Markdown mode → Editor, terminal lock → Terminal); grab-bag panes are forbidden; a one-setting domain becomes a section; when the task fixes a layout the child keeps it and lists "Placement suggestions" instead.

---

## Iteration 8

### Plan

Skill v7 (placement precision: object-named panes, no grab-bag panes, one-setting domain → section, fixed layouts win). Same eight scenarios.

### Do

Skill snapshot: [`runs/i08/skill.md`](../runs/i08/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i08/`](../runs/i08/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 582 s | 97 | see report |
| S2 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 492 s | 49 | see report |
| S3 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 379 s | 74 | see report |
| S4 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 403 s | 37 | see report |
| S5 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 541 s | 93 | see report |
| S6 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 299 s | 33 | see report |
| S7 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 153 s | 55 | see report |
| S8 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 338 s | 25 | see report |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S6 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S7 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S8 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |

**S1** — `default-apple-dark-en.png`, `rest-picker-apple-dark-en.png`, `selected-top-left-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `hover-picker-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-picker-apple-dark-en | selected-top-left-apple-dark-en |
|---|---|---|
| ![](shots/i08/s1/default-apple-dark-en.png) | ![](shots/i08/s1/rest-picker-apple-dark-en.png) | ![](shots/i08/s1/selected-top-left-apple-dark-en.png) |


**S2** — `default-apple-dark-en.png`, `hover-choice-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `icons-minimum-apple-dark-en.png`, `icons-maximum-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-choice-apple-dark-en | focus-choice-apple-dark-en |
|---|---|---|
| ![](shots/i08/s2/default-apple-dark-en.png) | ![](shots/i08/s2/hover-choice-apple-dark-en.png) | ![](shots/i08/s2/focus-choice-apple-dark-en.png) |

- S2: Placement suggestions listed instead of moving the fixed layout (L10 held).

**S3** — `default-apple-dark-en.png`, `focus-avatar-switch-apple-dark-en.png`, `hover-theme-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i08/s3/default-apple-light-ja.png) | ![](shots/i08/s3/default-apple-light-ja-720x692.png) | ![](shots/i08/s3/default-apple-dark-en.png) |


**S4** — `default-apple-dark-en.png`, `focus-alert-volume-apple-dark-en.png`, `focus-mute-apple-dark-en.png`, `hover-play-apple-dark-en.png`, `focus-output-apple-dark-en.png`, `volume-min-apple-dark-en.png`, `volume-middle-apple-dark-en.png`, `volume-max-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-alert-volume-apple-dark-en | focus-mute-apple-dark-en |
|---|---|---|
| ![](shots/i08/s4/default-apple-dark-en.png) | ![](shots/i08/s4/focus-alert-volume-apple-dark-en.png) | ![](shots/i08/s4/focus-mute-apple-dark-en.png) |

- S4: Self-reported: section headings collapsed until the child widened its wrappers — the third occurrence across S4/S5; traced to the kit `section` title column (fixed as F4).

**S5** — `default-apple-dark-en.png`, `hover-card-apple-dark-en.png`, `focus-help-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-card-apple-dark-en | focus-help-apple-dark-en |
|---|---|---|
| ![](shots/i08/s5/default-apple-dark-en.png) | ![](shots/i08/s5/hover-card-apple-dark-en.png) | ![](shots/i08/s5/focus-help-apple-dark-en.png) |


**S6** — `default-apple-dark-en.png`, `hover-card-apple-dark-en.png`, `pressed-card-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `disabled-card-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-card-apple-dark-en | pressed-card-apple-dark-en |
|---|---|---|
| ![](shots/i08/s6/default-apple-dark-en.png) | ![](shots/i08/s6/hover-card-apple-dark-en.png) | ![](shots/i08/s6/pressed-card-apple-dark-en.png) |


**S7** — `default-apple-dark-en.png`, `focus-name-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-name-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i08/s7/default-apple-dark-en.png) | ![](shots/i08/s7/focus-name-apple-dark-en.png) | ![](shots/i08/s7/default-apple-light-en.png) |


**S8** — `default-apple-dark-en.png`, `pane-appearance-apple-dark-en.png`, `pane-graph-apple-dark-en.png`, `pane-smart-commit-apple-dark-en.png`, `pane-editor-apple-dark-en.png`, `pane-terminal-apple-dark-en.png`, `pane-network-apple-dark-en.png`, `pane-general-apple-dark-en.png`, `pane-analyze-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | pane-appearance-apple-dark-en | pane-graph-apple-dark-en |
|---|---|---|
| ![](shots/i08/s8/default-apple-dark-en.png) | ![](shots/i08/s8/pane-appearance-apple-dark-en.png) | ![](shots/i08/s8/pane-graph-apple-dark-en.png) |

- S8: Panes: Appearance, Graph, Smart Commit, Editor, Terminal, Network, General (language), Analyze — no grab-bag pane (L9 held); one-setting Editor/Network panes kept because S8 names them (L10 applied, self-reported).
- S8: Evaluator: Custom themes rows repeat their title in their button ("Open theme folder" / "Open theme folder…") — a writing defect the skill does not cover.

Every scenario met every checklist item — the first fully clean iteration. L9 and L10 held: S8 produced object-named panes (Appearance with Reduce motion, Graph with ⌘C copies, Editor, Terminal, Network) and General holds only Language; children working on fixed layouts listed placement suggestions instead of moving settings.

After this iteration the owner pointed out a rule the skill and kit lacked: **controls placed side by side share one height**. Measured in the kit itself, row controls were 22 pt (stepper buttons), 24 pt (pop-up, segmented) and 26 pt (capsule) — a kit-level inconsistency that no checklist item measured.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S2** accessibility role needs a stateful element (GPUI API; resolved by the child).
- (self) **[Execution] S4** section headings collapsed inside wrappers; root cause in the kit `section` (title column without `flex_1`).
- (self) **[Planning] S5** HIG corpus unavailable — environment.
- (self) **[Planning] S8** one-setting panes vs the scenario's named panes — handled by L10.
- (evaluator) **[Formatting] S8** row titles repeated in their buttons.
- (owner) row controls of different heights — no rule, and the kit itself mixed 22 / 24 / 26 pt.

### Act

Foundation **F4**: one `metrics::CONTROL_HEIGHT` (24 pt) for every trailing control — capsule, stepper buttons, checkbox, section action now match pop-up and segmented; `section` keeps its width (title column `flex_1`, accessory does not shrink).

Skill **v8** (owner request, theme "row alignment"): §6 critique adds **"Same row, same height"** — a pop-up and its button, segments, swatches, cards in a grid row (as tall as the tallest), capsules in a button row share one height and centre line, measured in the PNG; §2 says page-local row controls use `CONTROL_HEIGHT`.

---

## Iteration 9

### Plan

Skill v8 (same row, same height) + foundation F4 (one `CONTROL_HEIGHT` for every row control; `section` keeps its width).

### Do

Skill snapshot: [`runs/i09/skill.md`](../runs/i09/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i09/`](../runs/i09/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 433 s | 31 | see report |
| S2 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 367 s | 65 | see report |
| S3 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 250 s | 24 | see report |
| S4 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 393 s | 77 | see report |
| S5 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 390 s | 30 | see report |
| S6 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 112 s | 50 | see report |
| S7 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 548 s | 64 | see report |
| S8 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 560 s | 96 | see report |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S6 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S7 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S8 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |

**S1** — `default-apple-dark-en.png`, `rest-picker-apple-dark-en.png`, `selected-bottom-right-apple-dark-en.png`, `selected-top-left-apple-dark-en.png`, `selected-top-right-apple-dark-en.png`, `selected-bottom-left-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `hover-picker-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-picker-apple-dark-en | selected-bottom-right-apple-dark-en |
|---|---|---|
| ![](shots/i09/s1/default-apple-dark-en.png) | ![](shots/i09/s1/rest-picker-apple-dark-en.png) | ![](shots/i09/s1/selected-bottom-right-apple-dark-en.png) |


**S2** — `default-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-choice-apple-dark-en | focus-slider-apple-dark-en |
|---|---|---|
| ![](shots/i09/s2/default-apple-dark-en.png) | ![](shots/i09/s2/focus-choice-apple-dark-en.png) | ![](shots/i09/s2/focus-slider-apple-dark-en.png) |


**S3** — `default-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i09/s3/default-apple-light-ja.png) | ![](shots/i09/s3/default-apple-light-ja-720x692.png) | ![](shots/i09/s3/default-apple-dark-en.png) |


**S4** — `default-apple-dark-en.png`, `focus-alert-volume-apple-dark-en.png`, `hover-play-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-alert-volume-apple-dark-en | hover-play-apple-dark-en |
|---|---|---|
| ![](shots/i09/s4/default-apple-dark-en.png) | ![](shots/i09/s4/focus-alert-volume-apple-dark-en.png) | ![](shots/i09/s4/hover-play-apple-dark-en.png) |

- S4: Speaker symbols still unavailable in the icon catalog (kit/catalog gap).

**S5** — `default-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `hover-manage-apple-dark-en.png`, `focus-navigation-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-card-apple-dark-en | hover-manage-apple-dark-en |
|---|---|---|
| ![](shots/i09/s5/default-apple-dark-en.png) | ![](shots/i09/s5/focus-card-apple-dark-en.png) | ![](shots/i09/s5/hover-manage-apple-dark-en.png) |

- S5: Self-reported: section titles sit 1 px off the row inset in Apple Light (group border) — kit defect, fixed as F5.

**S6** — `default-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | default-apple-light-en |
|---|---|
| ![](shots/i09/s6/default-apple-dark-en.png) | ![](shots/i09/s6/default-apple-light-en.png) |


**S7** — `default-apple-dark-en.png`, `focus-name-apple-dark-en.png`, `focus-swatch-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-name-apple-dark-en | focus-swatch-apple-dark-en |
|---|---|---|
| ![](shots/i09/s7/default-apple-dark-en.png) | ![](shots/i09/s7/focus-name-apple-dark-en.png) | ![](shots/i09/s7/focus-swatch-apple-dark-en.png) |

- S7: Cursor Text / Selected Text reuse term_bg / term_fg (no dedicated theme fields) — data gap recorded.

**S8** — `default-apple-dark-en.png`, `pane-appearance-apple-dark-en.png`, `pane-graph-apple-dark-en.png`, `pane-editor-apple-dark-en.png`, `pane-network-apple-dark-en.png`, `pane-terminal-apple-dark-en.png`, `pane-smart-commit-apple-dark-en.png`, `pane-analyze-apple-dark-en.png`, `pane-language-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | pane-appearance-apple-dark-en | pane-graph-apple-dark-en |
|---|---|---|
| ![](shots/i09/s8/default-apple-dark-en.png) | ![](shots/i09/s8/pane-appearance-apple-dark-en.png) | ![](shots/i09/s8/pane-graph-apple-dark-en.png) |

- S8: Evaluator: "Reload Themes" row still repeats its title in its button (second occurrence of the writing defect).

All checklist items ○ in all eight scenarios for the second iteration in a row. No heading collapsed anywhere (F4), and row controls line up: pop-up + Play, stepper buttons, capsules and checkboxes now share 24 pt. Remaining findings are small and specific: a 1 px title inset difference in Apple Light (self-reported by S5), and S8 repeating a row title in its button again.

#### Unclear points (self-reported + evaluator)

- (self) **[Execution] S1** a 720 pt-high narrow capture clipped the last row — handled by L4 (taller capture).
- (self) **[Understanding] S4** no speaker icon — catalog gap (unchanged).
- (self) **[Execution] S5** header inset vs bordered light group → kit F5.
- (self) **[Understanding] S7** no dedicated Cursor Text / Selected Text theme fields — data gap.
- (evaluator) **[Formatting] S8** title repeated in its button (second time) → L11.

### Act

Foundation **F5**: section titles add the light group's 1 px border to their inset. Skill **v9** (theme: label wording, L11): a row's control never repeats the row's title; an action-only row is a single capsule.

Iteration 10 is the last planned round, so the hold-out H (Rex Themes, never shown to any child) runs alongside S1–S8 to check over-fitting.

---

## Iteration 10

### Plan

Skill v9 (label wording, L11) + foundation F5 (title inset in light). Last planned iteration: S1–S8 plus the hold-out H (Rex Themes), which no child had seen.

### Do

Skill snapshot: [`runs/i10/skill.md`](../runs/i10/skill.md). Fresh children, one per scenario; prompts and reports in [`runs/i10/`](../runs/i10/).

| Scenario | Model (resolved) | Options | Wall time | Timeline items¹ | Self-reported retries |
|---|---|---|---|---|---|
| S1 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 426 s | 90 | see report |
| S2 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 438 s | 40 | see report |
| S3 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 263 s | 50 | see report |
| S4 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 377 s | 30 | see report |
| S5 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 366 s | 65 | see report |
| S6 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 365 s | 39 | see report |
| S7 | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 435 s | 73 | see report |
| S8 | `codex/gpt-6.1-sol` | reasoningEffort=high, serviceTier=default | 412 s | 41 | see report |
| H | `codex/gpt-5.6-terra` | reasoningEffort=high, serviceTier=default | 346 s | 66 | see report |

¹ T3 does not report tool-use counts for delegated children; the child thread's timeline item count is a proxy, not comparable with Task-tool `tool_uses`.

### Check

| Scenario | Success | Accuracy | Item scores |
|---|---|---|---|
| S1 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S2 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S3 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S4 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S5 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S6 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○ |
| S7 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| S8 | ○ | 100% | 1=○; 2=○; 3=○; 4=○; 5=○; 6=○; 7=○ |
| H | ○ | 92% | 1=○; 2=○; 3=○; 4=△; 5=○; 6=○ |

**S1** — `default-apple-dark-en.png`, `rest-picker-apple-dark-en.png`, `selected-bottom-left-apple-dark-en.png`, `focus-picker-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `hover-nav-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | rest-picker-apple-dark-en | selected-bottom-left-apple-dark-en |
|---|---|---|
| ![](shots/i10/s1/default-apple-dark-en.png) | ![](shots/i10/s1/rest-picker-apple-dark-en.png) | ![](shots/i10/s1/selected-bottom-left-apple-dark-en.png) |


**S2** — `default-apple-dark-en.png`, `hover-choice-apple-dark-en.png`, `pressed-choice-apple-dark-en.png`, `focus-choice-apple-dark-en.png`, `disabled-choice-apple-dark-en.png`, `focus-switch-apple-dark-en.png`, `focus-slider-apple-dark-en.png`, `icons-min-apple-dark-en.png`, `icons-max-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-choice-apple-dark-en | pressed-choice-apple-dark-en |
|---|---|---|
| ![](shots/i10/s2/default-apple-dark-en.png) | ![](shots/i10/s2/hover-choice-apple-dark-en.png) | ![](shots/i10/s2/pressed-choice-apple-dark-en.png) |

- S2: Self-reported: S2 never states the slider range; the child declared 1–5 (scenario ambiguity, not a skill issue).

**S3** — `default-apple-dark-en.png`, `focus-auto-fetch-apple-dark-en.png`, `default-apple-light-en.png`, `default-apple-light-ja.png`, `default-apple-light-ja-720x692.png`

| default-apple-light-ja | default-apple-light-ja-720x692 | default-apple-dark-en |
|---|---|---|
| ![](shots/i10/s3/default-apple-light-ja.png) | ![](shots/i10/s3/default-apple-light-ja-720x692.png) | ![](shots/i10/s3/default-apple-dark-en.png) |


**S4** — `default-apple-dark-en.png`, `focus-alert-slider-apple-dark-en.png`, `focus-mute-apple-dark-en.png`, `hover-play-apple-dark-en.png`, `volume-minimum-apple-dark-en.png`, `volume-middle-apple-dark-en.png`, `volume-maximum-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-alert-slider-apple-dark-en | focus-mute-apple-dark-en |
|---|---|---|
| ![](shots/i10/s4/default-apple-dark-en.png) | ![](shots/i10/s4/focus-alert-slider-apple-dark-en.png) | ![](shots/i10/s4/focus-mute-apple-dark-en.png) |


**S5** — `default-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-card-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i10/s5/default-apple-dark-en.png) | ![](shots/i10/s5/focus-card-apple-dark-en.png) | ![](shots/i10/s5/default-apple-light-en.png) |


**S6** — `default-apple-dark-en.png`, `hover-card-apple-dark-en.png`, `focus-card-apple-dark-en.png`, `scroll-middle-apple-dark-en.png`, `scroll-end-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | hover-card-apple-dark-en | focus-card-apple-dark-en |
|---|---|---|
| ![](shots/i10/s6/default-apple-dark-en.png) | ![](shots/i10/s6/hover-card-apple-dark-en.png) | ![](shots/i10/s6/focus-card-apple-dark-en.png) |


**S7** — `default-apple-dark-en.png`, `focus-name-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-name-apple-dark-en | default-apple-light-en |
|---|---|---|
| ![](shots/i10/s7/default-apple-dark-en.png) | ![](shots/i10/s7/focus-name-apple-dark-en.png) | ![](shots/i10/s7/default-apple-light-en.png) |


**S8** — `default-apple-dark-en.png`, `pane-appearance-apple-dark-en.png`, `pane-graph-apple-dark-en.png`, `pane-editor-apple-dark-en.png`, `pane-smart-commit-apple-dark-en.png`, `pane-terminal-apple-dark-en.png`, `pane-network-sync-apple-dark-en.png`, `pane-general-apple-dark-en.png`, `pane-analyze-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | pane-appearance-apple-dark-en | pane-graph-apple-dark-en |
|---|---|---|
| ![](shots/i10/s8/default-apple-dark-en.png) | ![](shots/i10/s8/pane-appearance-apple-dark-en.png) | ![](shots/i10/s8/pane-graph-apple-dark-en.png) |

- S8: Custom themes is one "Theme folder" row plus a capsule row (Open theme folder… / Reload Themes / Theme file guide…) — no title repeated (L11 held).

**H** — `default-apple-dark-en.png`, `focus-new-apple-dark-en.png`, `focus-import-apple-dark-en.png`, `default-apple-light-en.png`

| default-apple-dark-en | focus-new-apple-dark-en | focus-import-apple-dark-en |
|---|---|---|
| ![](shots/i10/h/default-apple-dark-en.png) | ![](shots/i10/h/focus-new-apple-dark-en.png) | ![](shots/i10/h/focus-import-apple-dark-en.png) |

- H item 4 item: partial — swatch card is reusable with a selected ring, but "Default" and "Built-in" are separated by a dot, not the divider the reference uses
- H: Hold-out: the cards fill their previews with saturated invented colours instead of quiet theme-like previews — weaker than S6, but every checklist item but one holds.

S1–S8 met every checklist item for the third iteration in a row. L11 held (S8 now writes one "Theme folder" row and a capsule row of actions). The hold-out scored 92 % (one partial item) against a recent S1–S8 mean of 100 %: inside the protocol's 15-point band, so no over-fitting signal — but its cards are visibly weaker than S6's, which had the real theme data to draw from. A data-driven card is easy to make good; an invented one depends on the implementer's taste, which the skill does not yet guide.

#### Unclear points (self-reported + evaluator)

- (self) **[Understanding] S2** slider range unspecified by the scenario.
- (self) **[Understanding] S4** meaning of a JA capture for an English scenario (the child ran an extra capture).
- (self) **[Planning] S8** one-setting panes vs named domains — L10 applied again.
- (self) **[Planning] S5** HIG corpus unavailable — environment.
- (evaluator) **[Planning] H** invented preview palettes are loud; no rule says how to fake a preview when no data exists.

### Act

Stop: iteration 10 reached (protocol). Convergence by the strict rule was **not** reached — iterations 8–10 each had new, mostly environmental or scenario-level unclear points — but accuracy plateaued at 100 % on S1–S8 for three iterations and the hold-out stayed within 15 points. Open items carried to import: a rule for previews without real data (H), the icon catalog's missing symbols (speaker, cloud), and keyboard wiring for kit controls.

---

## Final check — independent blind review and repository verification

### Blind image review (cross-family)

`claudeAgent/claude-opus-5-5` looked only at the iteration-10 captures and
the reference screenshots — no source, skill, reports or scores — and
reviewed every part role across pages and themes (#1798 method). It looked
at the default dark/light captures, the S3 Japanese captures and four focus
states; the other hover/pressed/value states were not reviewed, so
state-only defects may be missing. Report:
[`../blind-review-i10.md`](../blind-review-i10.md).

- Result: **C1 = 10, C2 = 20, overall 3 / 5.** Rows, group cards, headings,
  pop-ups and capsule heights line up across pages ("the kit skeleton works;
  the light set reads close to System Settings").
- What the same-model evaluation missed, grouped:
  1. **Focus, selection and "on" share one accent treatment** (a focus ring on
     a yellow switch, a focused choice tile that looks selected).
  2. **Page-local cards break most** (theme-card previews with low contrast,
     a wrapped name, cut-off fifth card; iCloud tiles nested inside a card).
  3. **Nested cards** (tile grids and tables inside a group card).
  4. **Sidebar items drift between pages** (one tile size, glyph per item).
  5. **Theme differences** (selected segment darker than its track in light,
     black switch knob in dark, light-only hairline shifting layout by 2 px),
     and Japanese line-break rules (a line starting with "。").
- This is the method's "same-model blind spot": Astra scored 100 % on these
  pages because the fixed checklists do not measure state distinctness or
  cross-page consistency. The review becomes the next iteration's input
  rather than a re-score of the frozen checklists.

### Repository verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets` | exit 0; no warning in `settings_kit`, `gallery_host` or `settings_gallery` (66 pre-existing warnings elsewhere) |
| `cargo build` | exit 0 |
| `cargo test --workspace` (private `KAGI_LOG_DIR`) | exit 0 — 3,902 passed, 0 failed |
| `uv run --project ci check-all` | exit 0 (LOC: no new file over 800) |

Production Settings is unchanged (the experiment only registers two new
public modules), so no Tier A settings scenario changes; Tier B evidence for
the new surfaces is the gallery captures above.
