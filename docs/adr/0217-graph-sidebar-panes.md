# ADR-0217: Graph sidebar の 5 ペインと保存する高さ比

- Status: Accepted
- Date: 2026-10-01
- Related: #864、ADR-0014（Repository Navigator）、ADR-0197（window-global sidebar state）、ADR-0199（sidebar gesture）

## 背景

Graph の Repository Navigator は local branch / remote branch / worktree / tag / stash を 1 本の仮想 list に平坦化していた。多数の local branch があると後続の section を見るために list 全体をスクロールする必要があり、それぞれの一覧を独立した高さにできなかった。PR は上部の PRs タブで扱い、Graph には重複させない。

## 決定

1. **5 枠を固定**: Graph の sidebar に local branch → remote branch → worktree → tag → stash の順で、見出しを固定した 5 つの Tree を表示する。PR は上部の PRs タブだけに置く。filter と Merged branches は上に置く。worktree inspection は行にホバーした時だけカードに表示し、下部に常設ペインを置かない。各 Tree の葉だけ独立した `UniformListScrollHandle` で仮想スクロールする。既存のクリック / context menu / drag & drop / group collapse と accessibility の階層・兄弟位置は、共通の行 renderer を使い続ける。
2. **行の単一キャッシュと密度**: `render` は従来の fingerprint で `sidebar.rows` を構築し、その再構築時だけ 5 個の連続 range を求める。スクロールや divider drag は行を clone / group 化し直さない。section header と通常の leaf は 20px（section の文字は xs）、group の見出し内容は 18px、worktree leaf はその pane 専用の 24px。通常の leaf は xs、相対時刻などの補助文字は zoom に連動する 11px 程度。`uniform_list` は各 pane 内で同じ行高を要求するため、group は 20px の slot 内に収め、worktree leaf のみ別 pane の 24px slot にする。pane geometry は paint 済みの bounds を持ち、root の `DividerDrag::SidebarPane(index)` が cursor の window 座標と zoom から、両側で最も近い開いた 2 枠の重みだけを変える。閉じた枠の高さは見出し分で固定し、開くと以前の重みに戻る。狭い viewport に見出し 5 個が収まらない場合は外側の stack を縦スクロールする。
3. **状態の所有者**: 既存の `SidebarState::collapsed: HashSet<&'static str>` だけが runtime の開閉状態。保存形式の mask はこの集合から毎回導出し、起動時の `SidebarState::new()` で一度だけ復元する。幅・filter・開閉・重み・スクロールは tab 切替でコピーしない window-global presentation state（ADR-0197）。Git repository に書き込まない。
4. **設定は 1 キーの文字列**: `settings.json` の `"sidebar_panes": "v1:3500,2500,2000,1000,1000:0"` は local 35% / remote 25% / worktree 20% / tag 10% / stash 10% の 5 個の正の `u16` 重みと下位 5 bit の開閉 mask。未リリースの旧 6 重み値は件数違いとして既定値にフォールバックし、raw は上書きしない。`Settings::sidebar_pane_layout()` は欠落・壊れた版・件数違い・0・範囲外を既定値として読むだけで、起動・再描画で壊れた raw を上書きしない。ユーザーが divider を動かすか header を開閉したときだけ、**現在有効な重みと新しい mask の完全な v1 値**を `settings::write_setting` に渡す。`settings::store` が既存の原子的保存と drag の連続書き込み集約を担当する。別の設定 key は増やさない。
5. **WORKTREES は linked のみ**: main worktree は Graph の worktree glyph / WIP から引き続き開けるが、削除対象ではないため sidebar に葉を描かない。見出しの件数は表示する linked worktree 数とし、容量計測の対象からも main を外す。`nonconcurrent` の port block はこれまでどおり main の read model から取得して linked の行に表示する。右クリック時は、root の menu より後に描かれる GPUI の hover tooltip を一時的に描かず、既存の操作 menu を前面に保つ。削除操作をホバーだけでは表示しない。

## 検証

- pure test: 最初の pane が LOCAL で section と連続 range の順序が一致する。設定の 5 重み v1 往復と旧 6 重み・不正値・0・件数違いの既定値フォールバック。
- native Tier A `sidebar_tree_roles` / `sidebar_panes` / `worktree_inspection`: 5 つの名前付き Tree、Graph に PR pane がないこと、実マウスの separator drag、独立スクロール、開閉と重みの復元、不正値の起動時不変と header 操作後の v1 書き込み、900px と 600px / 125% の配置・スクロール、再 mount 後の状態復元。
- #911 review: divider の重みは header を含む pane 全体の実測高と同じ座標系で計算する。高さの異なる LOCAL / REMOTE の境界を初回の小さな実マウスドラッグ後、separator の中心が pointer と一致することを Tier A で検証する。旧 6 ペイン時に header 分を差し引く計算では PR / LOCAL が 12px ずれることを変異で確認した。

## 却下した案

- pane ごとに branch / tag 等を再分類・再 clone する: divider drag と再描画が O(全 ref) になる。
- 開閉用の第 2 key / 独立した bool 配列: `SidebarState::collapsed` と保存値が乖離する。
- viewport が低いとき pane を自動で閉じる: 明示的なユーザーの開閉状態を変更してしまう。

## Amendment — 2026-10-12: first-frame filter entry (#1309)

The window-owned filter InputState is created by the shared window entity
constructor, before the first frame, rather than by clicking an inert
placeholder. Its gpui-kit Input retains dense sidebar sizing and uses Home's
localized filter wording and clear affordance. Tab reaches it immediately;
Cmd+F focuses it in Graph, and Escape clears it and returns to the local
list's first available stop (the header when empty/collapsed). This does not
change the window-global ownership in decision 3 or copy state on tab switches.

Initial review clarification (#1309, superseded by #1331 below): only the
gpui-kit Input carried TextInput; its immediate Group ancestor carried the
EN/JA filter name because the pinned Input had no direct accessible-name API.
The placeholder uses a language cache and updates the retained state before
drawing after a language switch.

## Amendment — 2026-10-13: focused Input names (#1331)

The fork's `Input::aria_label` API puts the localized EN/JA accessible name on
the actual focused TextInput, including the sidebar filter. This supersedes
the #1309 Group-ancestor naming workaround: remove naming-only Group wrappers,
but retain genuine container semantics. Every gpui-component Input uses the
same direct naming API and existing localization conventions; no second
unfocused TextInput is introduced. Filter ownership, first-frame availability,
keyboard behavior and language-switch placeholder updates remain unchanged.

## Amendment — 2026-10-12: horizontal pane budget (#1355)

Reference: Apple HIG Layout's primary-content priority; retain Kagi's dense
sidebar/Inspector, existing divider interactions and zoom tokens, not a new
control or navigation mode. The outer pane budget follows Graph's inner-column
fit (#1317): derive render widths, shrink Inspector first and sidebar second,
and never replace preferred widths merely because the window or zoom changed.
Widening restores them. Inspector and Compare share this fitted right-slot
width. Closing/opening clips retain the full fitted content width.

| Region | Preferred reference px | Drag preference minimum | Presentation minimum |
|---|---:|---:|---:|
| Sidebar | 240 | 120 | 120 |
| Inspector / Compare | 360 | 240 | 160 |
| Centre beside Inspector / Compare | remaining | n/a | 320, zoom-scaled |

At 940×660 / 167%, the old 120 + 240 + 320 minima plus two 4px dividers
require 1034.064 screen px at the 1.503 physical scale. The presentation-only
160px Inspector floor makes the required 608 reference px fit in 913.824 screen
px without changing the existing 240px drag-preference minimum. If even both
presentation floors do not fit (for example at 700px / 167%), shrink Inspector
then sidebar to their floors and finally let the centre use the nonnegative
remainder; do not panic or assign a negative width.

Divider candidates are re-fitted against the last rendered budget before
changing a preference. An expansion already capped by fitting is ignored,
just as for #1317's Graph columns; a genuine resize retains the existing
reference-space preference semantics. Swipe distance uses the fitted sidebar,
not its preferred width. Commit Panel keeps its existing viewport policy.
Git, modal, keyboard and accessibility behavior are otherwise unchanged.

This is the fit for the non-collapsed layout. ADR-0224's central configured
rem boundary determines when auxiliary panes collapse; pane fitting does not
introduce another breakpoint.

Native Tier A `pane_budget_fit` retains EN/JA at 1000×660 / 167% using an
isolated supported 40rem threshold to keep the original desktop stress case.
It covers diff-header containment, exact unchanged wide-window edges at
1392×883 / 100%, capped divider drags and preference restoration after widening.
Dedicated compact scenarios cover the rail and threshold transitions. Pure unit
coverage retains the non-collapsed sub-minimum fitting fallback.
Neighbor scenarios are `diff_`, `graph_`, `sidebar_panes`, `inspector_`.
These geometry/input assertions are not native VoiceOver or Tier B evidence.

Inspector's complete-message regression retains simultaneous containment for
trailers shorter than the message viewport. A value that wraps taller than the
viewport at the fitted width must expose its bottom and then its top through
the same native wheel scroller; requiring the whole oversized row visible in
one frame would instead force truncation or invalidate this valid layout.

Review clarification: deferred worktree keyboard cards, group identity reveals
and Branch Cleanup column drag origins must all use the fitted sidebar width.
Using the preferred width for those local anchors detaches them from the
painted sidebar after compression. `pane_budget_worktree_anchor` and
`pane_budget_group_anchor` assert the real card/reveal left edge against the
painted sidebar's content edge at 940×660 / 167%, without changing preferences.
Local card/reveal anchors subtract `keyboard_nav::RING`: RowList always lays
out its transparent 2px focus border, so its local content origin is inset
even before the ring is visible. This keeps the actual card edge on the
sidebar edge rather than accepting a larger geometry-test tolerance.

## Amendment — 2026-10-13: keyboard collapse and filtered disclosures (#1449, #1450)

The five panes retain exactly one Tab stop each: a roving drawn row, or the
header when collapsed/empty/header-only. Outline Left first collapses a focused
expanded group through `toggle_sidebar_group` (inert with a reason while
filtering); on its child, Left reveals and focuses the immediate parent group.
Only a top-level row that is not an expanded group collapses the pane and
focuses its remaining header immediately. A worktree inspection card holding
focus is explicitly excluded, so Left on its controls changes neither pane,
card nor focus. Enter/Space on the pane header
use the same `SidebarState::toggle_section` as a pointer click, including for
an expanded empty pane. Opening still transfers focus to the first drawn row;
an opened pane without room for a row retains header focus. The existing
`persist_panes`, collapse mask and weights remain the only layout authority;
collapse never changes weights or Git data.

**Filtering policy: dynamic group disclosures are inert with a localized
reason.** Matching local-prefix, remote and remote-subgroup children remain
automatically expanded while a filter is nonempty. Pointer, Enter/Space and Left
activation report “Groups stay expanded while filtering. Clear the filter to
collapse a group.” in EN/JA through the existing informational toast.
Neither activation changes the session's normal `branch_groups_collapsed`
set; clearing the filter therefore restores exactly the user's normal choice.
This deliberately chooses the inert-policy option rather than a second
temporary collapse set: it preserves the existing complete filter results,
full-name tooltip/reveal and single collapse owner. Section/pane collapse is
separate and remains available during filtering.

Tier A `sidebar_keyboard_collapse` covers all five populated/empty/collapsed
panes, Left/Enter/Space, one-stop navigation, focus return, saved mask/weights
and remount, plus expanded-group collapse and child-to-parent focus legs for
local prefixes, remotes and remote subgroups. `sidebar_keyboard_collapse_card`
keeps inspection actions/card/focus intact on Left in the same matrix.
`sidebar_filtered_disclosure` covers each dynamic group from
open/closed normal states with repeated pointer and keyboard activation,
localized reason, children/chevron/AX agreement and clear restoration.
Both use real Git fixtures and assert zero Git writes across 940×660 and
1392×883, 100%/167%, EN/JA. These renderer/input assertions are not native
VoiceOver, Full Keyboard Access or Tier B evidence.

## Amendment — 2026-10-25: retain keyboard stash inspection focus (#1761)

Stash Peek is read-only inspection, not navigation away from the stash tree.
Enter/Space therefore retain the focused stash's OID-keyed row in both the inline
sidebar and compact navigator Popover; keyboard context-menu Peek returns to that
same row. Keep the compact stash Popover open for those keyboard activations.
Pointer leaf selection and other compact navigation actions retain their existing
dismissal behavior. The `stash_row_identity` matrix checks Enter, Space and menu
Peek after an external stash shifts the displayed indices, without Git writes.

