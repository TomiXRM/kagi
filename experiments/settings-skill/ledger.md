# Failure-pattern ledger

One entry per `General Fix Rule`. Before adding an entry, check whether an
existing rule already covers the issue: a repeat means the earlier fix sits in
the wrong place in the skill (too late, too weak, or not where the
implementer looks), so move or rewrite it instead of appending.

| ID | General Fix Rule | Phase | First seen | Fix location (skill version) | Recurrences |
|---|---|---|---|---|---|
| L1 | A visual self-check must name concrete per-part criteria (indicator on its value, same role same look, one trailing edge, both themes); "compare with the reference" finds presence, not geometry. | Execution | i01 S2 (slider thumb off its track; self-check ○) | v1 §5 Critique | — |
| L2 | When the reference breaks a skill rule, the rule has to sit where the reference is used, and say that the rule wins; a rule read after the reference loses to it. | Planning | i01 S2 (Rex's multi-colour tiles copied over §3) | v1 §5 "§3 beats the reference" → moved in v2 into the §2 kit-table rows for sidebar row / icon tile / segmented | i02 S1 (non-accent sidebar tiles justified as a "status scheme") |
| L3 | A forced state shows the same data in another interaction state; a page must not change its data to make a state look different. | Understanding | i01 S1 (`position-rest` dropped the selected value) | v3 §2 "Interaction states never change data" (+ value states named for the value) | i03 S1 (`rest` = nothing selected, subtitle "Choose a position") |
| L4 | A capture is evidence only for what it shows: size the page (or capture each scroll position) so every element the checklist names is visible; an uncaptured item is never ○. | Execution | i02 S1 (last group below the fold in every capture) | v4 §1 size comment + §4 "A capture is evidence only for what it shows" | i04 S1 (same; plus visual items self-scored ○ after the host refused every capture) |
| L5 | A rule must state its scope where it is applied; a general "use X for N choices" rule collides with a scenario that asks for a different control for the same count. | Understanding | i02 S2 (segmented rule vs preview tiles, self-reported) | v2 §2 Segmented row | — |
| L6 | Give the implementer the closed vocabulary it must choose from (here: the available `IconName` variants); a guessed name costs a compile round each time. | Execution | i03 S1, S3 (unavailable icon names, self-reported) | v4 §2 IconName list | i04 S2 (unavailable icons, self-reported) |
| L7 | A rule about "the selected row" vs "every row" must name its subject; "its tile" was read as the selected tile only. | Understanding | i03 S2 (non-selected sidebar tiles neutral) | v4 §2 Icon tile row "Every sidebar tile — selected or not" | i04 S1, S3 |
| L8 | A page-local control derives every indicator from one value→position function; a wrapper around kit parts keeps `w_full()` / `min_w_0()`. Controls that recur as page-local gaps belong in the kit (F1). | Execution | i01/i05 S2 (sliders), i05 S5 (collapsed headings) | v5 §2 page-local rules + F1 kit Slider | i06 S4 (collapsed heading); none after i07 |
| L9 | Name panes for the object a setting changes; a pane named for a kind of setting (Behaviour, Advanced, Misc) is a grab bag; a one-setting domain is a section, not a pane. | Planning | i07 S8 | v7 §3 with worked moves | — |
| L10 | When the task fixes the layout, the task wins over a general placement rule; suggestions go in the report. | Planning | i07 S3 (self-reported) | v7 §3 last bullet | — |
| L11 | A row's title names the object and its control names the action or value; a control that repeats its row's title is redundant, and an action-only row is a single capsule. | Formatting | i08 S8 | v9 §4 | i09 S8 |
| L12 | Owner-taught layout basics (same row, same height) belong in the kit's constants first and the critique second: a kit that mixes 22/24/26 pt teaches every page to mix them. | Execution | owner after i08 | F4 `CONTROL_HEIGHT` + v8 §6 "Same row, same height" | — |
