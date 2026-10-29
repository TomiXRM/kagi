# Settings skill experiment — protocol

Experiment branch `experiment/kagi-settings-skill` (2026-10-27). Question:
**can a fresh implementer, given only a Kagi skill and Kagi-owned settings
components, rebuild a Rex-quality settings page reproducibly?** The skill, not
the implementer's taste, is under test. Issues: #1798 (image critique), the
part gallery #1799, one owner per control #1800, and agentic UI evaluation
#1802.

Method: mizchi `empirical-prompt-tuning` (SKILL-ja.md, the URL version with
Trace phases, `Issue / Cause / General Fix Rule` retrospectives and a failure
pattern ledger).

## Roles

| Role | Who | Does |
| --- | --- | --- |
| Author / evaluator | Astra (`openai-codex/gpt-6-astra`, parent thread) | Writes the skill and the kit, scores every run from real-window captures and code, revises the skill. |
| Implementers | fresh T3 `delegate_task` children: `codex/gpt-6.1-sol`, `codex/gpt-5.6-terra` | Read only the task prompt (skill inlined), build one page, return the report. A child is never reused. |
| Cross-family reviewer | `claudeAgent/claude-opus-5-5` | HIG design review before code; blind image review at convergence. |

## Fixed inputs (never changed after iteration 1 starts)

- `scenarios.md` — three scenarios (S1 median, S2/S3 edge), one hold-out (H),
  each with a 3–7 item checklist and at least one `[critical]` item.
- References: Rex Beta Settings screenshots (Notifications, Interface, Themes,
  Server; outside git under `target/ui-lab/reference/`), Apple HIG Markdown
  pages (`target/ui-lab/hig/`).
- Capture matrix per run: Apple Dark EN 1.0×, Apple Light EN 1.0×; S3 adds
  Apple Light JA and a 720 pt narrow window.

## Per-iteration loop

1. **Plan** — write the iteration's hypothesis and the single theme of the
   skill change (or "baseline").
2. **Do** — dispatch one fresh child per scenario with the contract below.
   Children work in the shared checkout, write only
   `examples/lab_i<NN>_<scenario>.rs`, and must not modify shared files.
3. **Check** — Astra builds and captures each page with the gallery host
   (`KAGI_GALLERY_OUT`), reads the PNGs and the source, and scores every
   checklist item ○ / × / partial (0.5). Success = all `[critical]` ○.
   Record the child's self-report (unclear points with Trace phase,
   discretionary fills, retries) and runtime evidence (wall time from task
   timestamps; tool calls only when the platform reports them, else
   `unavailable`). Defects Astra finds in the captures or code that the
   child's self-check missed are recorded as *evaluator issues* with the same
   Trace phase and `Issue / Cause / General Fix Rule`; "new unclear points"
   in the stop rule counts both kinds (clarified after iteration 1, before
   iteration 2; no checklist item changed).
4. **Act** — each unclear point becomes `Issue / Cause / General Fix Rule`.
   Check `ledger.md` first: a repeat of a ledger rule means the previous fix
   was in the wrong place. Apply one theme to the skill (multi-source
   convergent findings may be batched). Kit/harness fixes are recorded
   separately as *foundation* changes because they confound the skill effect.
5. Child sources move to `runs/i<NN>/` after scoring (not compiled there).

## Stop rule

Converged when two consecutive iterations have zero new unclear points,
accuracy change ≤ 3 points, and then the hold-out H scores within 15 points of
the recent mean. Otherwise stop after iteration 10 and report the plateau.
Three iterations without fewer unclear points → rewrite the skill structure.

## Child contract (inlined in every dispatch)

```
You are a blank-slate implementer of <skill name>.
## Skill (verbatim)
<SKILL.md>
## Scenario
<scenario text from scenarios.md>
## Requirements checklist
<items, with [critical] tags>
## Task
1. Implement the scenario following the skill. 2. Verify as the skill says.
3. Reply with the report structure.
## Report
- Deliverable / evidence (commands run, PNG paths)
- Requirement self-check ○ / × / partial with reasons
- Unclear points: each with phase (Understanding / Planning / Execution /
  Formatting) and Issue / Cause / General Fix Rule
- Discretionary fills
- Retries (count, reason)
```

## Evidence handling

Screenshots are never committed (AGENTS.md). They live in
`experiments/settings-skill/log/shots/` (git-ignored) and `target/ui-lab/`;
the PDCA log links them relatively so the log renders in a local checkout.
