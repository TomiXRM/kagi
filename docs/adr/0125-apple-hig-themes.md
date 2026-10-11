# ADR-0125: Apple Light / Apple Dark themes (HIG system colors)

- Status: Accepted
- Date: 2026-07-19

## Context

kagi のテーマは W9-THEME(ADR-0036)の `Theme` トークン機構に載っている。
ユーザー要望: Apple の Human Interface Guidelines のシステムカラー
(https://developer.apple.com/design/human-interface-guidelines/color)から
Apple Light / Apple Dark の 2 テーマを追加したい — ボタン・文字色から
スイムレーンのパレットまで HIG 由来で。

HIG のシステムカラーは 2025-06-09(Liquid Glass 対応)で**値が更新**されて
いる。ページは SPA のため、docs データ JSON
(`/tutorials/data/design/human-interface-guidelines/color.json`)の
スウォッチ画像 alt テキスト(`R-255,G-56,B-60` 形式)から 12 色 ×
{Default, Increased contrast} × {light, dark} と `systemGray`..`Gray6` を
抽出した(2026-07-19 取得。例: red light は旧 `#FF3B30` → 現 `#FF383C`)。

## Decision

1. `crates/kagi-ui-core/src/theme_apple.rs`(sibling、LOC ratchet 配慮)に
   `APPLE_LIGHT` / `APPLE_DARK` を定義し `THEMES` に登録
   (slug: `apple-light` / `apple-dark`)。
2. **色の使い分けポリシー**:
   - Dark テーマは *Default (dark)* variant をそのまま使う(暗背景向けに
     調律済み)。
   - Light テーマは、細線・テキストとして読まれるもの(ステータス色・
     change バッジ・**スイムレーン 8 色**)に *Increased contrast (light)*
     variant を使う(default の yellow `#FFCC00` は白地の 2px レーンとして
     判読不能)。塗りチップ(ref バッジ)は鮮やかな *Default (light)*。
   - HIG の色意味論を保存: blue = アクセント/リンク、green = 成功、
     red = 破壊的、orange = 警告。
   - 背景/テキストは `systemBackground` + `systemGray` ランプ +
     label/secondaryLabel の実効(α合成済み)値。
3. スイムレーンの並びは既存パレットと同じ隣接最大差の順
   (pink→green→blue→orange→teal→purple→yellow→cyan、ADR-0104)。
4. ターミナル 16 色は normal = 一方の variant、bright = もう一方で構成
   (light: contrast/default、dark: default/contrast)。

## Consequences

- テーマ数 11 → 13。既定(index 0 = Catppuccin Mocha)は不変。設定・メニューは
  slug ベースなので挿入位置の影響なし。
- HIG の値が再改訂された場合は `theme_apple.rs` の定数を更新する
  (取得手順はこの ADR の Context に記載)。

## Amendment (2026-10-08): neutral macOS chrome and readable labels

Issues #1065 / #1066: Apple Light の灰色に色味があり、sidebar の見出し・更新時刻や
フォームの label が薄い。当初の公式 illustration と RGB 値だけの評価では、
実 Finder に対して sidebar を暗くしすぎた。ユーザーの再指摘を受け、同じ Mac で
空の検証フォルダを開いた Finder の実ウィンドウを撮影して比較した
([PR #1085](https://github.com/TomiXRM/kagi/pull/1085))。両方とも非 key window、
Light 表示、透明度低減・コントラスト増加は無効。ユーザーの foreground と
既存ウィンドウは変更していない。Finder の操作モデルや material の再実装ではない。

Apple Light の背景は iOS の `systemGray` 表の転記ではなく中立色にする:
sidebar / panel `#f7f7f7`、hover / divider surface `#e5e5e5`、
alternate row `#f5f5f5`、content `#ffffff`。更新先は現在の
`crates/kagi-ui-core/src/theme_apple_light.rs` の既存 token のみ。

撮影画像の Display P3 profile を native decoder で sRGB に変換した空白点では、
Finder の sidebar / toolbar は `(249,249,249)`、content は `(255,255,255)`。
旧案の Kagi sidebar は `(241,241,241)` で明度が低かった。更新後は sidebar /
toolbar が `(249,249,249)`、content が `(255,255,255)` となることを実画像で確認した。
source token と撮影 pixel の値は区別する。これは現在の非 active 状態の比較であり、
別 wallpaper / display / OS material 状態でも一致すると主張しない。

読む必要がある補助文字は `text_muted = #666666`、
`text_sub = text_label = #606060` とする。palette 値の sRGB relative
luminance から計算した contrast は、最も暗い対象 surface `#e5e5e5` でも
それぞれ 4.558:1 / 4.992:1、sidebar では 5.360:1 / 5.870:1。
disabled の opacity と装飾には通常文字と同じ要件を適用しない。

Apple Dark、ほかの built-in preset、Git の状態色・ref chip・lane palette、
font、geometry と操作契約は変更しない。custom theme の明示値は保持され、
`extends: "apple-light"` で省略した token だけは更新後の値を継承する。
これは静的 preset の調整であり、OS の dynamic semantic color や vibrancy を
導入したとは扱わない。

色の階層は実 Finder と比較して改善したが、Kagi の太い pane divider・大文字の
group heading・inactive window の文字/操作の減衰・Inter と system font の差は
残る。今回の palette 修正を「Finder と同じ native control/material」と評価しない。
font の別監査は #1068。custom override の検証で使った緑色 sidebar は preset と
混同させないよう、実機検証終了時に通常 Apple Light に戻す。


## Amendment — 2026-10-11: readable body links (#1077)

Apple Light's vivid `color_branch = #0088ff` remains a filled ref/Primary
accent; it is 3.520:1 against the white body background and is not the body-link
foreground. A separate `link = #0066cc` maps to TextView's `colors.link` and
gives 5.567:1 on white. Apple Dark's existing yellow link is already readable
and remains unchanged. Underline, activation, selection and geometry are not
altered.

## Amendment — 2026-10-11: system chrome accent (#1181)

Closes #1181.

On macOS, built-in Apple Light defaults to the person's system accent for
primary chrome, focus rings and text selection. Apple Dark defaults to its
theme accent (`#ffd600` yellow), preserving ADR-0126's identity; System is an
explicit opt-in on Dark. `Theme::ui_accent()` is distinct from the Git
`color_branch` role: ref chips, graph lanes, status colours, terminal and body
links keep their existing palette. This supersedes ADR-0126's non-goal of
separating chrome from branch colour, not its yellow default.

The startup and window-activation seams read `AppleAccentColor` with the
in-process, read-only `CFPreferencesCopyAppValue` global-domain API. The key is
retained per UI thread; there is no activation-time subprocess. Cache the
detected enum and derived fill tokens; no native lookup or allocation occurs in
a row/render. Refresh on activation is deliberately used rather than installing
an AppKit notification observer or adding an Objective-C dependency. Changing
Appearance and returning to Kagi updates the existing bridge and redraws.
Both numeric and string-stored values are accepted; strings are trimmed and
parsed as `i32`, matching AppKit's handling of e.g. the string `"1"` as Orange.
Null, other property-list types, invalid or unsupported values use Default (Blue).

The standard macOS accent palette ([HIG Color](https://developer.apple.com/design/human-interface-guidelines/color))
is kept separately from the graph's increased-contrast/2025 system colours:

| Default | Name | Light | Dark |
|---|---|---|---|
| -1 | Graphite | `#8e8e93` | `#98989d` |
| 0 | Red | `#ff3b30` | `#ff453a` |
| 1 | Orange | `#ff9500` | `#ff9f0a` |
| 2 | Yellow | `#ffcc00` | `#ffd60a` |
| 3 | Green | `#34c759` | `#30d158` |
| 4 | Blue | `#007aff` | `#0a84ff` |
| 5 | Purple | `#af52de` | `#bf5af2` |
| 6 | Pink | `#ff2d55` | `#ff375f` |

In System mode, missing/unsupported Mac values (including Multicolour) use blue
and are labelled Default (Blue), rather than falsely identifying an explicit
Blue preference. Settings → Appearance has an Apple-only **System / Theme accent**
choice (JA: 「システム」/「テーマのアクセント」), persisted as
`accent_source: system|theme`. An absent source uses System for Apple Light and
Theme for Apple Dark; an explicit value applies to either Apple preset.
Theme restores the preset's fixed chrome (`#0088ff` Light, `#ffd600` Dark) and
selection tint. The detected Mac name remains visible in EN/JA in either mode.
Reading migrates legacy `accent: blue` to `accent_source: theme` and legacy
`accent: system` to `accent_source: system`, then removes the old key. A `blue`
value under the new key is canonicalized to `theme`; the new key takes precedence.
Other presets, custom themes including Apple-derived themes, and non-macOS
platforms retain their fixed accent hue and do not follow macOS.

Primary labels reuse the bridge's contrast-preserving foreground/hover/pressed
derivation (4.5:1 minimum), including the update pill and filled thread badges.
Accent-as-text now routes through `accent_text_on` on every theme for its actual
background instead of the raw fill RGB, including busy footer text and conflict
mode labels. Fixed-accent themes keep their accent hue but small text may become
darker or lighter to achieve at least 4.5:1. This is a shared readability policy,
not system-follow for other themes. Text selection retains the
shared 30% alpha; Apple Light row selection derives a 15% accent wash on white,
while Apple Dark keeps its neutral row selection. `selected_for_window(true)`
uses this resolved wash; inactive selection keeps its independent neutral token.
Pane divider hover/drag uses this same chrome accent, on both the centered
hairline and its larger hit target; Git colour tokens are not used for either.
Ring geometry stays at the existing fixed 2 logical px. Unit coverage includes
all seven chromatic accents **and** graphite in both appearances and readable
text on base/surface/selection backgrounds. Tier A `system_accent_chrome` injects
blue/purple/graphite/red/yellow in memory and checks native primary/ring/graph-bar
and active/inactive selection pixels, retained selection, and stable
control/label bounds.

## Amendment — 2026-10-11: quiet Graph navigator group headers (#1177)

The five pinned section headers use localized title-style labels (English
Title Case, existing Japanese terminology), `text_sub`, 12 reference-px
semibold text, a separate small chevron and a neutral `bg_base` count pill
retaining `(N)`. Their 28 reference-px height is a 12px line with 8px padding
above and below. This changes only top-level header geometry: virtualized
body slots remain 20px (ordinary/group rows) and 24px (worktrees).
Collapsed panes and divider minimums use the header height rather than a body
slot. Apple Dark and other presets reuse their existing secondary-label and
neutral-surface tokens; no palette, global border, Git-state or accent changes.
The existing collapse owner, focus wrappers, persisted mask and AX expanded
state remain in place. This is hierarchy/geometry, not native material parity.
Token-derived sRGB contrast for the heading is 5.870:1 on Apple Light's
sidebar and 4.992:1 on hover; Apple Dark is 6.631:1 and 4.863:1 respectively.
Native `sidebar_panes` records inherited size/weight/colour at paint, plus
header/label bounds to assert the vertical inset at 100%/167% in EN/JA.
Review correction: the SVG sets its own `text_sub` colour (GPUI SVG painting
does not inherit the parent's colour), label/pill spacing uses margins rather
than a flex gap so the non-E2E empty probe cannot add spacing, and the pill's
`bg_base` fill remains distinct from the header's `surface` hover.
## Amendment — 2026-10-11: pane hairlines (#1174)

Dense Graph navigation takes Finder's continuous sidebar and quiet separator
hierarchy, not Finder's material or navigation model. Resting resize boundaries
paint a centered literal `px(1.)` `surface` line over the adjacent `sidebar`,
`panel` or `bg_base` fill. Existing scaled 4px hit regions, independent pane resizing,
collapse rules and drag handlers remain intact. Hover retains the accent
affordance only during interaction; collapsed inactive section handles remain
neutral. Keyboard focus, empty/loading/error states add no new interaction:
existing section focus and pane state contracts remain unchanged.

The same paint/target separation applies to PR, conflict, Editor and File
History pane boundaries, the bottom panel and the inspector message/files split.
PR navigator uses `sidebar`; PR swimlane and Conflict dividers use `bg_base`.
GPUI suppresses ordinary hover styles during a drag, so same-kind `drag_over`
preserves the target's accent. Group hover/drag-over also colors the line,
avoiding a grey stripe through the accented handle; unique groups scope each
handle. The pinned group API cannot inspect `DividerKind`, so crossing another
handle during a different-kind drag can transiently accent only its 1px line,
not its full target. Resize handlers and geometry remain unchanged.
Apple Dark and Color Vision use their own existing tokens, with no palette or
gpui-component global border change. Native `hairline_dividers` covers Graph
line bounds and rendered resting/hover/drag colors at 100% and 167% in both Apple
themes; existing sidebar/divider scenarios retain the resizing oracle.
## Amendment — 2026-10-11: non-key selection (#1176)

Selection presentation reads GPUI window activation, with an observer requesting
repaint on activation changes. `inactive_selected` is independent of the active
accent: Apple Light `#d3d3d3`, Apple Dark's existing neutral `#3a3a3c`, other
presets distinct neutral steps. Primary labels must meet 4.5:1 on this fill.
Graph commit/WIP rows (including accent-bar dimming), conflict/CommitPanel/
Inspector/PR file rows, repository/Home/bottom tabs, toolbar workspace-mode
buttons, sidebar, Home/filter/mode selectors and component list selections
retain selected IDs and geometry. Git status, ref and lane colours remain semantic.
Selection differs from every surrounding base/sidebar/alternate/hover role by
at least 1.15:1, except Latte hover (1.14 floor to preserve its primary-label
contrast); measured ratios and the exception are recorded in `docs/decisions.md`.
The native regression overrides only the activation input under `gui-e2e`,
measuring rendered Metal pixels without taking the user's foreground.
Pane-focus attenuation is not conflated with window activation in this change.

## Amendment — 2026-10-11: neutral structural separators (#1175)

Toolbar group hairlines, timeline/PR-table row boundaries and the command-palette
frame/search divider use `surface` at 1 logical px. `selected` remains a row
state, and `text_muted` remains readable secondary text. The shared gpui-kit
`colors.border` bridge is not changed: markdown tables depend on its stronger
muted tint. No new control, focus behavior or selection palette is introduced.

## Amendment — 2026-10-11: identifying input boundaries (#1183)

Apple Light's resting input/unchecked-checkbox boundary is `#8e8e8e`, owned by
the Apple Light palette and mapped only to `colors.input` in the theme bridge.
Its calculated sRGB contrast is 3.276:1 on white and 3.058:1 against the adjacent
`#f7f7f7` panel. `surface` is suitable for decorative separators, not an identifying
control boundary. Labels, placeholder/body colours, focused accent ring, 1px
outline, radius 6 and field sizes are unchanged. Apple Dark and custom themes
retain their current input-boundary mapping to preserve existing contrast.
The shared boundary also applies to Default-outline buttons (including modal
Secondary, inspector/oplog/conflict actions), Radio, Select, Combobox, NumberInput
and DatePicker. Keep one `colors.input` role rather than special-casing buttons.

## Amendment — 2026-10-11: system scrollbar visibility (#1182)

The PR table keeps its single horizontal viewport and independent virtualized Y
list from #1095. `Scrolling` preserves auto-hide and overlays the full viewport.
gpui-kit maps non-auto-hide macOS settings to `Hover`; both `Hover` and `Always`
therefore reserve a 16px lane and pin this table's scrollbar to `Always`, so its
track hitbox cannot steal bottom-row clicks. Global scrollbar colours and
keyboard/wheel routing are unchanged.

## Amendment — 2026-10-11: configurable compact toolbar (#1178)

Finder's default icon-only presentation is the default for Apple Light and
Apple Dark; other active themes retain Icon and Text. The flat string preference
`toolbar_display` accepts `icon_only` and `icon_and_text`; an explicit value
overrides theme changes, while an absent/invalid value follows the active theme.
Settings → Appearance and the View menu share the settings persistence owner.
The existing action row is 36 reference px in icon-only mode with 20px glyphs;
Icon and Text retains its 52px row and existing caption geometry.
No captions are retained exceptionally: distinct action glyphs, localized
tooltips and AX names identify every action (rationale in the decision log).
Unavailable reasons, count overlays, group hairlines, mode highlighting and
keyboard focus rings keep their existing owners. This reuses the existing SVG
pipeline, not SF Symbols, native material or a replacement action dispatcher.

Review correction: caption mode uses the same localized short label as its
AX name and default tooltip (history tooltips retain a localized label prefix).
View → Toolbar is declared in `MENU_BAR` as `DynSubmenu::ToolbarDisplay`;
native menus and the Linux/FreeBSD inline dropdown share localized entries,
selection markers and the `SetToolbarDisplay` action, per ADR-0085.
## Amendment — 2026-10-11: neutral document tabs (#1180)

The repository and Home strip takes Finder/Safari's neutral document-tab
hierarchy, not native materials or a new tab-navigation model. Apple Light uses
`bg_base` active / `surface` inactive; Apple Dark uses `surface` active / `bg_base`
inactive, so inactive cells remain visible against the `panel` strip and its 1px gaps.
Both keep `text_main` labels in key/non-key windows. Selection stays distinct from
the independent keyboard focus ring; it does not require accent attenuation.
Other presets retain their selection/surface palette, activation behavior and
active accent rail: some existing fill pairs have <1.15:1 contrast without it
(One Light 1.010:1, Flower Road 1.082:1, Tokyo Night key 1.099:1, Latte non-key
1.142:1). Only Apple themes remove the permanent 2px top rail. All presets remove
worktree-wide tint and stored lane rank; the existing 🌲 identity marker remains.
Close is visible for active or focused
tabs and on inactive-tab hover, with a reserved target so reveal never changes
label width. Cmd+W, keyed identities, one Tab stop, manual activation, tooltips,
40px strip and 80–200px cell limits remain unchanged. A single tab is not hidden.
Native `repo_tabs_neutral` measures fills, top-edge rail absence, inherited
label colours and bounds for 1/2/6 tabs, long JA names, 100%/167%, both Apple
themes and the activation seam. Real pointer/AX delivery remains Tier B.
The role-level unit test requires every preset without a rail to have
active/inactive fill contrast ≥1.15:1 and each primary label ≥4.5:1. Other
presets are asserted against existing selected/surface roles rather than
recoloured. The native gap probe asserts strip-background pixels differ from
inactive fills. The rail probe samples `bounds.top() - RING + 0.5`, including
the old overlay above the content bounds; restoring it was measured to fail
with actual blue `[0,136,255]` instead of white `[255,255,255]`.
## Amendment — 2026-10-11: persistent conflict file selection (#1187)

The dense Conflict Dashboard keeps the selected file's window-aware theme fill
even while the pointer is over that row; hover is only a state of an unselected
row. Both Conflicted Files and Resolved Files expose named ListBoxes with
selected options. Existing navigation buttons (pointer or keyboard activation)
select and open the same file, including clearing the content editor when the
destination is binary. Selection is not inferred from a display name.

Resolving an in-memory draft keeps its file open and selected in Resolved Files
for review; it does not unexpectedly switch away while editing. Next / Previous
conflict then wrap to an unresolved file, retaining the existing fallback to an
adjacent file when none is unresolved. Save's authoritative re-detection retains
the existing path-based fallback when a file leaves the session. No selection
palette, geometry, repository-write or logging contract changes.

Native `conflict_file_selection` reproduced the bug before the fix: after a
measured click on `notes.txt`, the rendered fill was `#f5f5f5` (hover), not the
required inactive `#d3d3d3`. Afterward the same assertion passes, as do active
selection under hover, selected AX attributes, dashboard Next, editor Previous /
Next, keyboard Space on Next, binary-preview cleanup and resolved-draft selection.
The fixture's repository fingerprint remains unchanged. `inactive_selection`,
`conflict_continue_cache` and `conflict_pane_survives_activation` also pass.
These are native renderer / attribute-recorder assertions, not VoiceOver or
Tier B evidence.

## Amendment — 2026-10-11: bottom-panel pane selectors (#1205, #1206)

Operation Log / Terminal / Activity reuse `keyboard_nav::TabList`: one Tab
stop, Left / Right / Home / End move focus, Enter / Space activate, Tab leaves.
Activation is manual because Terminal may start a process. Rounded, compact
cells are similar to the sidebar mode selector, not a shared styling helper:
the bottom selector keeps its own 28px height and window-aware selection fill.
Visible/AX names are localized in EN/JA. The existing fixed focus ring
is independent of selection. Selected cells keep `selected_for_window` under
hover; only unselected cells receive the `surface` wash. Closing the panel
returns focus to the app root rather than leaving focus on an absent cell.
BottomTab ownership, lazy terminal startup, Cmd+J and existing logging remain
unchanged. This is shared Kagi chrome, not AppKit material or native controls.

Native `bottom_panel_tab_hover` failed before the cutover with rendered
`#e5e5e5` hover instead of inactive `#d3d3d3` selection. Afterward it passes
Apple Light/Dark × EN/JA × 100%/167% with active/inactive hover paint, named
Tab attributes, one selected cell/Tab stop, label containment, manual key
activation, unchanged shell spawn count on arrow focus, distinct accent-ring
pixels, one-step Tab departure and focus release on close. Lazy Terminal
startup's focus transfer is restored to the keyboard cell; otherwise the next
Right/Space goes to Terminal rather than the selector. `bottom_panel_nested`
and `bottom_panel_motion` also pass. These are native renderer and attribute
recorder assertions; live VoiceOver / real-window comparison remain Tier B.

Review correction: `TabList` selection callbacks receive keyboard versus
pointer activation. Keyboard Terminal activation restores selector focus;
pointer activation retains Terminal's PTY focus instead of returning to the
root. The native matrix asserts this pointer route explicitly. The scenario
restores in-memory theme/language/zoom before unmount as well as saved keys.
## Amendment — 2026-10-11: dialog push buttons and named input tasks (#1208/#1209)

Cancel uses the existing Default-outline Secondary push button, not Ghost
text. Dialog actions retain S height and right-aligned Cancel-before-default
order; Primary follows `ui_accent`, Destructive uses the filled danger family,
and identifying secondary boundaries follow the theme bridge's `colors.input`.
Hover, press, focus-visible and sizing remain owned by gpui-kit Button.
Unavailable confirms remain inert and expose their localized reason.

The shared input-dialog frame reuses `apply_dialog` for a named modal Dialog
and availability-aware Confirm/Cancel actions, plus gpui-kit's existing focus
trap. Create Branch, Tag and Worktree, Stash Push, Rename/Set Upstream,
Worktree Lock Reason, Editor filesystem prompts, Clone, Remote Browse and
PR Fields use it. Existing plan and destructive cards retain their richer
Dialog/AlertDialog task boundary rather than receiving a nested outer Dialog.
Localized EN/JA titles and the single modal slot remain authoritative.
The existing InputState marked-text and Escape handling is not replaced.

Native `dialog_a11y_roles` failed against the pre-fix sources because Create
Branch had no recorded dialog boundary. Restoring only Ghost Cancel against the
fixed frame failed its native border pixel: `247` rather than required `142`
on Apple Light. The unchanged assertions pass with Secondary: matching action
heights, Cancel before default, border pixels, Primary accent and destructive
red pixels in Apple Light/Dark × EN/JA × 100%/167%. Opening focus and repeated
Tab/Shift+Tab remain in Create Branch, and Escape closes it. The destructive
card retains AlertDialog and its two-stage actions. Nine scoped neighboring
scenarios also pass, including marked-text Enter, rejected replan, queue release,
Editor/Remote/PR focus and Operation Log restoration.
These are native renderer/attribute-recorder assertions; live VoiceOver and
Tier B comparison remain the integration owner's responsibility.

Review correction: the filled danger family now shares Primary's
`filled_button_foreground` and `filled_button_interaction_colors` derivation,
preserving the resting blocker fill while ensuring label contrast ≥4.5:1 in
every state and distinct hover/pressed feedback across all 13 built-in themes
(including Color Vision). The regression failed before correction on Mocha's
2.32:1 fixed-white label; UI-core tests pass afterward.
Clone Escape is attached to the actual outer task focus owner, not a child
wrapper, and all clone cancellation paths schedule root-focus restoration.
Remote Browse's custom default action is Connect when idle in Connect, or
Open Repository when idle on a repository in Browse; busy tasks offer only
Cancel. The native tests retain raw Escape from clone-owner focus and check
the EN/JA Remote action availability.
## Amendment — 2026-10-11: layout-owned toolbar overflow (#1210)

The existing centered toolbar retains its natural button/caption geometry in
both `toolbar_display` modes. A canvas inside the toolbar measures its own
available width; canvases inside its controls record natural widths into a
fixed-size window-owned cache, normalized to the current zoom and keyed by
presentation, language and zoom. A key change remeasures natural elements
before fitting the next frame. These are production layout probes, not
window-bounds guesses or E2E recorder reads.
Measurements that change request the next frame; steady paints allocate no
fit vectors and do not notify. This two-pass choice keeps translated labels
and wide-window geometry authoritative rather than estimating text advances.
The row clips intermediate paint while measurements settle.

Fitting reserves existing group spacing and the equal side columns, with
room for Refresh and the repository identity. Repository commands overflow
first; workspace-mode/Analyze/Settings controls collapse last, and the selected
workspace control is the last candidate. Hidden commands omit their interactive
subtree entirely: GPUI still registers Tab stops when an interactive element
merely has `display: none`. Growing the row restores commands from cached
natural widths. The trailing » reuses the header's
`keyboard_nav::focusable` named, tooltip-bearing, ringed Tab stop. The pinned
Button cannot independently name a captionless AX node, so the existing
header helper draws the trigger rather than an unnamed icon Button.
gpui-kit Popover/PopupMenu owns positioning and menu keyboard navigation.
The header's existing controls are individual Tab stops, not a roving
`TabList`, and this preserves that convention.

Popover's mouse-down trigger does not receive GPUI keyboard clicks, so the named
trigger explicitly opens controlled state on Enter/Space release. Pointer clicks
still belong to Popover and are not handled twice. Closing returns the prior focus
only if focus still belongs to the menu (or nothing is focused); action-assigned
focus such as Terminal's PTY is retained. Widening away a focused overflow control
hands focus to the root so subsequent app shortcuts still dispatch. The cached
state holds the popover alive for synchronous dismissal before removing its
element; a next-frame weak dismissal can be too late to unregister gpui-kit's
deferred context and would block unrelated text-input right-click menus.

State contract: rest inherits the toolbar panel, hover/open use neutral `surface`,
the glyph uses `text_main`, and focus-visible uses the header's accent ring.
Unavailable rows remain reachable
to explain their localized reason, expose disabled AX state/description and invoke
only the existing reason footer. Empty overflow has no trigger; loading remains
the existing operation strip, and Git errors remain in the existing plan/footer
path. Workspace selection and all theme palettes are unchanged.

While a new style's natural widths are pending, retain the last hidden mask
intersected with commands still present. Invisible, noninteractive width probes
measure hidden controls only until their new natural widths are known. This avoids
temporarily dismissing an open menu or losing trigger focus at each zoom/language
change. Menu content is cached against the hidden-mask/reasons snapshot and
rebuilt when it changes, so labels and unavailable reasons do not remain stale.
Gaps and separators require visible neighbours on both sides; the fitting model
uses these same predicates rather than reserving dangling separators.

Popup rows retain localized labels, unavailable reasons and action-bound
shortcut glyphs. Activation uses the same toolbar dispatcher as the ordinary
buttons, including reason footers and Terminal's tab-specific toggle. Git
planning, confirmation, logging and theme-bridge colour roles are unchanged.
The native `toolbar_overflow` scenario covers 940×660 at 167% and 1392×660 at
100%, EN/JA and both modes with long repository/branch names: exact hidden/menu
membership, visible control containment, real toolbar Tab traversal with no
hidden Tab stops, Enter opening the menu, refreshed Undo reason during an open
language/zoom transition, retained Terminal PTY focus, Create Branch activation,
and a real text-input right-click path after widening an open menu away.
The existing count oracle retains every visible glyph's
size/containment/non-overlap assertion; hidden glyphs must instead be present in
the actual overflow menu. Real keyboard/VoiceOver delivery and Apple Light JA
caption-mode visual comparison remain Tier B.
## Amendment — 2026-10-11: quiet Settings and radio dots (#1199, #1201)

Settings uses the same secondary-label hierarchy as Graph group headers:
`text_sub`, 12 reference-px semibold. Frame, titlebar and section boundaries
are literal 1 logical px in `surface`, independent of system accent or zoom.
Control fills and focus continue through the existing accent/theme bridge.

The pinned gpui-kit Radio previously reused the checkbox check SVG. Its
selected mark paints a centered contrasting circle approximately 6px inside a
14px radio, scaled with the control; unchecked radios have no mark. Disabled
marks retain reduced opacity. Existing RadioGroup selection, focus, keyboard,
pointer and AX state ownership are unchanged. This is component paint, not a
hand-made replacement control or a native AppKit implementation.

Native regressions failed before the change: `settings_quiet` recorded the
accent-coloured heading instead of `text_sub`; `settings_radio_dot` sampled
the selected circle centre as `[0,122,255,255]`, not white. After the cutover,
both pass. Quiet Settings coverage includes EN/JA, both Apple themes,
100%/167%, injected blue/purple, secondary-label contrast ≥4.5:1 and adjacent
pixels enforcing literal 1px section/titlebar separators. The radio probe
checks symmetric white-centre and surrounding accent pixels plus unchanged
empty input fill in both themes/zooms. Fork head
`13386100eaca38a5759f69fe4596b59f2dff5141` was pushed and matched the remote
`fix/radio-dot` head before pinning both Cargo patch entries and the lockfile.
Real pointer/keyboard/VoiceOver delivery remains PM-owned Tier B evidence.

Review correction: the dot uses `primary_foreground` through the theme bridge,
not hard-coded white: Dark's yellow Theme accent requires a dark dot, as do
other light fills. The native glyph matrix includes explicit Theme accent and
blue/purple System accent at both zooms, comparing the foreground pixels and
requiring ≥4.5:1 contrast. The white-only fork failed with actual
`[255,255,255,255]` versus expected `[0,0,0]`.
Fork head `879f3bfa4c7d0cdfd2b91a7f290e1fce744c7f6b` supersedes `13386100`.

The shared non-E2E `measure_inside` placeholder is absolute and zero-sized,
so it cannot introduce a third flex item, a gap, or alter `justify_between`.
The E2E canvas branch is unchanged. A unit test failed on the old placeholder
(`position: None`, expected `Absolute`) and asserts both position and zero size.

Review validation: the default-build placeholder unit passes (1/1), and the
scoped native suite `settings_quiet,settings_radio_dot,settings_switches,settings_focus_trap`
passes (4/4), including settings isolation restoration of `accent_source`.
## Amendment — 2026-10-11: draggable surface scrollbars (#1202)

Settings' single body and Home's shared virtualized Repositories / PRs / Issues
list attach the pinned kit's vertical scrollbar to their persistent scroll owner.
Home Recent, Inspector files and message, plus Activity contributor ranking,
use the same policy. Operation Log already has a scrollbar and is not changed.
`Scrolling` overlays without reducing content width; `Hover` and `Always` reserve
a 16 logical-pixel lane and use `Always`, matching the PR table's #1182 policy.
No new preference, colour mapping, row allocation or wheel/key dispatcher is added.

Settings retains its application-wide ScrollHandle in KagiApp and resets it on
each open so the first keyboard control is visible. Inspector and Activity
positions belong to TabUiState and never carry another repository's offset.
Home Recent retains its application-wide handle in HomeGithub; the GitHub list
retains its existing ListState, cached entries, refresh anchoring and keyboard
row navigation. The pinned kit's ScrollbarState is private: GPUI retains it under
a stable scrollbar element id instead of application code allocating it each
render. Thumb interaction delegates to that kit, not a hand-built drag control.
Rest/hover/pressed visibility is the kit's system policy; empty/non-overflowing
content needs no thumb. Existing focus/AX list semantics remain unchanged.

Native regressions simulate mouse down/move/up on thumb geometry derived from
the measured production scrollbar region and owner extent, then assert changed
offset and repaint retention. Settings and Home target 940×660 Apple Light,
100%/167%, EN/JA, Scrolling/Always; Home reuses its 60-repository fixture.
Tier B must additionally inspect Settings wheel then thumb drag, EN/JA and
100%/167%, including last-section/list-item reachability; native state assertions
alone do not establish native material parity or VoiceOver delivery.

Audit exclusions: Operation Log already uses `with_vertical_scrollbar`;
PR navigator (`pr_nav.rs`), commit strip and file list (`pr_mode.rs`), Conflict
dashboard and context/modal scrollers (`conflict_view.rs`), command/menu
popovers and modal previews remain unchanged. They have distinct workspace or
transient-focus/layout owners outside Settings/Home and the specifically
requested Inspector/oplog/Activity audit; this change does not claim they all
have thumbs. Diff/editor/terminal scrollers already own independent rendering
and scrollbar behavior and are likewise not replaced.

Native scenario guards restore disk keys, in-memory zoom/language, active theme,
synced kit palette and scrollbar policy on Drop, including panic unwinding.
