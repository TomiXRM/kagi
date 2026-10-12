# ADR-0018: Status Bar Repository State Summary

- Status: Accepted / Date: 2026-06-12

## Decision
表示項目(左→右): branch / upstream 名(`→ origin/main`)/ ↑↓ / ● dirty / +staged / ~unstaged / **!conflict** / **⧉stash 数** / 最終 refresh 時刻 / **background operation(実行中の op 名 or external change 検知)** / 直近結果 / タブ icons。
0 件の数値は非表示(ノイズ抑制)。↑↓は Header と重複表示(判断材料のため意図的)。

## Consequences
- StatusBarSummary に conflict_count / stash_count / upstream_name / busy: Option<String> を追加

## Amendment — 2026-10-12: bounded branch identity (#1310)

The dense footer keeps its existing summary ordering and panel controls. Branch
identity has a minimum of the lesser of its measured natural width (rounded up
to match GPUI text layout) and a zoom-scaled 60-reference-pixel width, with end
ellipsis when compressed. Short names keep their natural size. Visible text,
Tooltip and AX share the complete
control-byte-sanitized branch name; raw repository identity and klog are unchanged.
Status chips (including ahead/behind and upstream) and the refresh timer retain
natural widths. Nonempty feedback reserves up to a zoom-scaled 80-reference-pixel
minimum with a zero flex basis, so branch identity absorbs excess width first.
The reservation is capped by the measured space remaining after fixed chips,
timer, controls, padding and the branch minimum (with 1px for layout snapping);
at 700px/167%, blindly adding both preferred text minima would clip the controls.
Each shaped chip/timer width is rounded up individually before calculating the
remaining room; a single final snapping allowance cannot replace those ceils.
Feedback remains a single-line truncated first-line preview before the
nonshrinking Terminal / Operation Log cluster. The original footer body and
durable Operation Log detail are unchanged.

This policy applies to both toolbar presentations, EN/JA and 100%/167% zoom.
Identity is a read-only label, not an additional Tab stop. Hover exposes its full
name; selected/pressed/disabled/loading/error input states are not applicable to
that label. Existing panel-button hover, toggled state, focus-visible rings,
keyboard activation and localized names remain owned by `panel_button`.

## Amendment — 2026-10-12: Busy feedback height (#1321)

The feedback slot is a horizontal flex row centered inside the fixed-height
footer. Busy retains the existing nonshrinking 10-reference-pixel animated
spinner, separated from its label by a scaled 4-reference-pixel gap. The label
alone has zero minimum width and end ellipsis; the feedback slot still retains
the #1310 minimum-width reservation above. Idle, Success and Failed use the same
single-line text slot without a spinner. Tooltip, full operation detail, colours
and reduced-motion behavior are unchanged. Native `status_bar_busy_height`
checks both height and complete containment at EN/JA and 100%/167% zoom.
