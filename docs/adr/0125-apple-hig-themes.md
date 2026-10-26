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

## Amendment — 2026-10-12: safe Return defaults (#1270)

For a destructive alert, Cancel is the default action: root Return and Escape
cancel without arming or executing. Danger styling does not imply a Return
default. This applies to Discard (including all), Amend, Stash Drop, local and
remote branch deletion, Reset, Force-with-lease Push, Replay, Branch Cleanup,
Remove/Prune Worktrees, Conflict Abort, Editor Discard/Delete, and any shared
plan card marked destructive (including Apply Suggestion, Sync to Remote and
History restoration). An armed unobservable-release AppNotice likewise cancels
on root Return, while its danger button becomes Tab-reachable. Guarded Rebase
remains non-destructive and single-stage: unlike Replay it retains the guarded
plan's existing Return = Confirm policy. There is currently no delete-tag
ActiveModal variant.

Confirm remains explicitly reachable by Tab followed by Space/Enter, pointer
activation or its named accessibility action. Focused buttons consume activation
keys before the root router, including Cancel on non-destructive dialogs.
Existing command-Return shortcuts are unchanged; none is introduced for a dialog
that did not already expose one. Non-destructive dialogs retain Return = Confirm.
Two-stage arming, unavailable controls, plan/preflight checks, marked-text input
protection and the single `active_modal` routing boundary remain unchanged.

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
room for Refresh and the repository identity. The original repository-first
collapse priority is superseded by #1316 below; the selected workspace control
remains the last candidate. Hidden commands omit their interactive
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

## Amendment — 2026-10-11: unified window frame (#1179)

**Historical layout, superseded by the #1462 owner decision below.** The unified
frame, below-toolbar document strip, single-document hiding, toolbar traffic-light
and + reservations, hidden-cell focus/TabPanel policy and 440.5px viewport result
in this amendment describe #1179, not the restored layout. The 52/60-reference-px
toolbar heights, neutral paint, existing command owners and blank-only window
gestures remain authoritative. Its native validation reports are historical,
not verification of #1462.

Every theme uses the same unified top-frame geometry, not an Apple-only fork.
GPUI still owns native macOS traffic lights, positioned at (9,16) logical px;
the frame reserves an unscaled 80px left exclusion. Repository name and branch
remain left, existing actions and » retain their dispatch/focus/AX owners, and
workspace controls stay right. Only blank spacers use `WindowControlArea::Drag`.
Colours continue through the existing panel/surface/accent bridge, not AppKit
material imitation or a new title state.

Icon Only has a 52-reference-px frame (46.8 logical px at 100%, below the 54px
target); Icon and Text uses 60 reference px (54 logical px at 100%).
The 20px glyphs, caption geometry and #1178 setting/menu paths remain unchanged.
#1210's production layout fitting additionally reserves the native exclusion
and trailing +; natural-width caching, overflow priority, keyboard menu and
unavailable reasons remain authoritative.

The document strip moves below the frame and is shown only when repository
tabs plus the optional Home tab total more than one. This explicitly supersedes
#1180's “A single tab is not hidden.” Home is still a document: one repository
plus Home shows both cells. The + affordance moves from the strip to the frame's
right end, including Home's frame; Cmd+T and File → New Tab open Home, Cmd+W and
File → Close Tab close the front document even with the strip hidden. Hidden
cells release keyboard focus to the root through the existing `TabList` owner.
Neutral fills, tab close/reveal rules and manual keyboard activation stay intact.

Native regressions intentionally update `toolbar-row` heights and single-tab
expectations; they measure first-frame ordering, exclusion/identity/action
containment, blank chrome region and the two-document strip below the frame at
1392×883, EN/JA, 100%/167%, Apple Light/Dark and Mocha. Neutral-tab paint and
existing toolbar/overflow/keyboard scenarios retain their behavioral oracles.
Real native traffic-light interaction, window dragging and the requested
Finder comparison remain integration-owned Tier B, not claimed by bounds probes.

Regression-first evidence: against the old sources, `unified_toolbar` failed
because the toolbar began at Y=36 instead of Y=0. After recomposition the
scenario passes its full matrix, including hidden-strip New/Close Tab commands,
frame + → Home and focus release when closing two documents down to one.
Scoped native verification passes all eight affected scenarios (six in the
first post-cutover selection, then `repo_tabs_neutral` / `keyboard_nav` after
deliberately updating their viewport and single-document focus expectations).
`ToolbarRestore` composes the existing presentation Drop guard, including
`GlobalSettings::capture()`, and restores cached toolbar display from its saved
key on unwind. Neutral fill/rail pixels and toolbar counts/reasons remain tested.

Review correction: workspace and Home share a single frame-height owner for both
display modes. The drag target is the actual, full-height blank spacer after
identity, not a container wrapping controls or a test-only band. macOS windows
set `app_owns_titlebar_drag: true`, disabling AppKit's enlarged native titlebar
drag band over the toolbar. A left press arms movement only in that blank spacer;
the following pressed move calls `window.start_window_move()`. Double-click on
the spacer calls `window.titlebar_double_click()`, honoring macOS's configured
titlebar double-click behavior (zoom/minimize/none); toolbar controls do neither.
Windows retains its native Drag hit test on the same blank spacers only.

Empty Home has no + when it cannot open another document. The no-tabs remote
route retains the folder-dialog action and uses Open Repository's localized
tooltip/AX name. As with the existing named overflow trigger, the + glyph's
node is drawn explicitly because the pinned Button derives AX names only from
visible labels. Cmd+T remains its keyboard route rather than adding a Tab stop.
`tab_strip_visible` gates both the strip and corresponding TabPanel roles:
hidden single-document panels are ordinary content. Native regression measures
production spacer bounds disjoint from every toolbar control, covers both display
modes ≤54px at 100%, equal Home/workspace heights and empty-Home + absence.
The original three-tab #961 close/reindex/Enter oracle is retained beside the
two→one focus-release check.

Review validation: the bounded native selection
`unified_toolbar,repo_tabs_neutral,keyboard_nav,toolbar_overflow,toolbar_count_scale,workspace_mode_toolbar,home_,repo_tab_panels`
passes 18/18 scenarios, including all 11 matching Home scenarios. The unified
matrix now includes Icon and Text and measures the real spacer rather than an
E2E-only drag proxy. Real AppKit drag/double-click delivery remains Tier B.

Integration geometry correction: the `commit_panel_viewport` desktop file-list
measurement becomes 440.5 logical px at 1392×883@100%, replacing 411.5px from
the separate strip/toolbar arrangement. The single-document unified frame gives
its flexible viewport more height; fixed composer (257px), Generate (21.5px),
Commit (25px), desktop panel width and small-body AutoGrow oracles are unchanged.
Recovery-test searches for the old list height, 36/52px chrome constants and
header offsets found no additional obsolete recorded-header-height expectation.
## Amendment — 2026-10-11: Home narrow-window containment (#1212)

Home is an open dashboard with a secondary Recent navigator. Like PR detail,
the content reader takes priority: reuse its 420-reference-px minimum, and
narrow Recent from 300 to 120 reference px when the ordinary sidebar leaves
less than that after Home's scaled 48-reference-px horizontal padding (matching
the existing `px_6` rem geometry). Compute compact mode once against the actual
sidebar width, including its test override, and pass it to repository rows. Keep Recent
mounted and reachable rather than inventing a disclosure, focus owner or
preference. Above the threshold, sidebar and centered 720-reference-px column
retain their existing geometry. The column explicitly fits viewport minus
sidebar rather than relying on percentage sizing against an indefinite flex
parent.
The 120-reference-px choice leaves a recognisable short-name navigator while
returning 180 reference px to the primary content. Long names/paths ellipsize;
the full path remains available through the row Tooltip and AX description.

Open Folder stays first when actions wrap. The existing TabList retains full
localized accessibility names and automatic arrow activation; its visible
labels shrink with ellipsis before Refresh can clip. Compact repository rows
reserve at least 40% for the name, truncate description and shrinking metadata,
and keep the final Clone/Open action fixed. Existing theme roles, read/cache
owners, keyboard navigation and Git write pipeline are unchanged. No new user
strings or hand-made controls are introduced.

The Home heading native scenario additionally checks narrow 940×660/167%
EN/JA control containment and repository-name priority, with 1392×660/100%
sidebar and column widths asserted against the recorded pre-fix native widths.
Tier B: Home, JA, Apple Dark, 940×660/167%.

Native pre-fix evidence: at 940×660/167% EN, SSH action bounds were
`x=706, width=338` (right edge 1044), outside the 940px viewport.
The unchanged 1392×660/100% renderer recorded a 269px sidebar interior
(270px frame minus its literal 1px border) and 648px centered column;
the regression asserts equality with those recorded widths in both languages.

Afterward, the scoped `home_heading` scenario passes: both EN and JA narrow
repository rows measure 687.5px, with a 301.5px name column (43.85%).
Folder, SSH, all three pane cells, Refresh, search, Recent and Clone bounds
remain inside the window. Widening restores the exact recorded 269px/648px
sidebar-interior/column widths. `cargo clippy -q -p kagi` reports no warnings
in changed files; existing unrelated warnings remain. The scenario uses
`GlobalSettings::capture()` for in-memory theme/language/zoom/toolbar restoration
on Drop, in addition to persisted-key restoration.

Review correction: date can shrink and ellipsize once badges have exhausted their
available width, rather than remaining an unshrinkable sibling of Clone. A
760×660/167% row-containment oracle failed with the old fixed date: Clone's right
edge was 739.5px against its row's 724px right edge. This additional cell checks
Clone against its row, not whole-screen containment; the complete-control
viewport oracle remains 940×660/167% EN/JA.
The scoped `home_` native suite passes all 11 scenarios after correction,
including the additional narrower row in EN/JA (507.5px row, 212px name
column, 41.77%). Existing Home reads, keyboard paging, list placement and
incomplete-search scenarios remain green.
## Amendment — 2026-10-11: discoverable status-bar panel controls (#1207)

The dense footer retains Terminal and Operation Log as its two bottom-panel
toggles, not inspector controls. Both reuse the header's named
`keyboard_nav::focusable` contract: individual Tab stops after content,
Enter/Space activation and the existing fixed accent focus-visible ring.
Localized stable Button names and state-dependent Show/Hide tooltips are
provided in EN/JA. `aria_toggled` maps Button's pressed state through the
pinned GPUI/AccessKit API, which has no `aria_pressed` setter.
Terminal's Hide tooltip advertises Cmd+J only while Terminal is active:
the shortcut toggles the panel without selecting a tab or starting Terminal,
so Show has no Cmd+J hint. Operation Log advertises focused Enter/Space
because no dedicated shortcut exists. The existing Cmd+J binding is unchanged.

The footer keeps its 22-reference-px height, with centered 16-reference-px
icons in 24-reference-px-wide targets and neutral `surface` hover. Resting
labels use existing primary/muted roles. No disabled/loading/error state is
introduced: these controls only select or hide the existing panel. Existing
lazy startup, panel ownership and logging remain authoritative. Keyboard
Terminal activation restores the status button's focus after lazy startup;
pointer activation retains the PTY focus. Native renderer/attribute and
keyboard assertions are not live VoiceOver or Tier B evidence.

Native `status_controls` failed before the fix with no recorded Terminal AX
name (`None`, expected `Terminal`). It now passes Apple Light/Dark × EN/JA ×
100%/167% at 600×660: named Button/pressed attribute inputs, state-dependent
tooltips, measured content-box containment in the footer, adjacent forward
Tab stops and symmetric reverse traversal, Enter/Space opening/closing the
corresponding panel exactly once, and pointer Terminal activation giving the
terminal view's focus handle the focus.
The bounds oracle accounts for the fixed 2px ring and native half-pixel
snapping. The shared presentation Drop guard restores disk keys and in-memory
language/zoom/theme/kit palette even on unwind; toolbar display is untouched.
## Amendment — 2026-10-11: bounded toolbar overflow detail (#1217)

The dense transient command menu keeps gpui-kit Popover/PopupMenu navigation,
focus and activation ownership. Labels and action-bound shortcuts share the
first line; the shortcut column does not shrink. The label column can shrink
to zero and its secondary unavailable reason truncates within that column.
Full reasons remain in AX descriptions and the ordinary refusal footer.
The menu width is bounded by the current viewport minus a 16 logical-pixel
edge margin on each side; resizing an open menu refreshes that limit.
The trigger has a tooltip only while closed. Japanese toolbar guard prose
is translated without English fallback clauses; ADR-0048 domain words remain
English (`upstream`, `commit`, `branch`, `pull`, `push`, `undo`, `redo`).
English messages are unchanged.
Rest, hover, disabled explanation and focus-visible roles retain the #1210
contract. This does not alter Git planning, logging or unavailable activation.
Tier B integration target: Apple Light JA 940×660 at 167%, both toolbar modes,
open » and inspect shortcut containment and absence of the trigger tooltip.

Native `toolbar_overflow` failed before the layout correction: a row starting
at Y=187 placed its shortcut at Y=201.5, centered across the reason instead
of on the first line. The same assertion passes afterward, along with actual
popup/row/window shortcut containment, viewport width cap, Japanese drawn AX
reason prose and no measured tooltip after hovering the open trigger for one
second. The matrix retains EN/JA, both display modes and 940×660@167% plus
1392×660@100%, with the existing Drop guard restoring global presentation.
This first-line failure is regression evidence, not a claim that Tier A
reproduced the issue's photographed glyph clipping; Tier B remains integration
owner evidence.

Review correction: Japanese reason tests reject English prose fragments rather
than domain vocabulary. The native tooltip assertion has a measured closed-state
positive control before checking its absence while open. A native resize from
940 to 480 logical pixels while open exercises the viewport-dependent 448px cap,
then restores the original width for the existing activation/focus coverage.
PopupMenu applies `max_w` to its inner items, not its bordered outer frame.
The cap subtracts the two literal 1px borders so the whole measured menu
fits within the viewport limit.
The custom row also has an explicit content-width ceiling that subtracts
the kit's rem-scaled `p_1` and fixed 8px item side padding: its intrinsic
wrapper otherwise allowed the newly restored long JA domain-word reason to
push a shortcut 2px outside the measured 500px menu. The strengthened native
scenario failed on that containment and the 480px shrink cap before these
corrections, and passes with both applied. Closed hover records a tooltip;
open hover records none in both languages and presentation modes.
## Amendment — 2026-10-11: Home repository reveal and actions (#1203, #1204)

Home remains an open-density list with single-line ellipsized descriptions.
The cached entry owns sanitized SharedString text reused by the visible label,
full-text Tooltip and row `aria_description`, preserving Japanese and literal
braces without converting descriptions on every repaint.

Clone/Open use the shared `KagiButton` Secondary (Default-outline) at 22
reference px, following macOS small-push-button density rather than the quiet
metadata chip. Its visible localized EN/JA label also names its Button AX node.
Private/Fork metadata remains `text_sub` on `surface`; no palette changes.
gpui-kit owns border, hover and pressed styles; RowList owns keyboard focus,
Enter/Space and the single list Tab stop. The button is not another Tab stop,
and child clicks stop propagation before invoking the existing pick once.
Loading/cloning continues through that existing pick/card owner; no Git-write,
logging or confirmation path changes. This is control hierarchy, not AppKit
material parity. Native renderer/recorder coverage and Tier B are distinct.

Native `home_repository_affordances` restores in-memory presentation via the
shared Drop guard and `GlobalSettings::capture()`. Restoring the old action chip
failed its measured height (17.5px versus 22px at 100%); removing the description
reveal and its observation failed with no recorded description. The description helper
sets `aria_description` and records that same argument on one call path; this is
an assignment-path assertion, not readback of GPUI's accessibility tree. Each
matrix cell clears old observations and consumes freshly built tooltip text.
The fixed matrix checks full EN/JA/braces text supplied on native hover,
constructor-supplied Button role/name, scaled height, rendered border distinct
from fill and metadata, and focused-row Space opening the existing clone card.
Attribute recorders do
not establish live VoiceOver delivery. Tier B: Home Repositories, Apple Light EN,
hover a truncated description and inspect the trailing action.

Integration with #1203/#1204 retains cached `Rc<RepoListing>` descriptions and
the nonshrinking Secondary action. Repository row state/compact presentation
are grouped at the renderer boundary, following the existing pane-cell tuple
convention rather than adding a Clippy suppression. The merged scoped `home_`
suite passes 12/12 scenarios; Clippy reports no warning in the row renderer.

## Amendment — 2026-10-11: Home virtual-list containment oracle (#1179, #1212)

Home can expose a partially mounted GitHub repository row below its scrollable
fold at 940×660/167%, independently of the chrome arrangement restored by #1462.
Native row bounds include virtualization overdraw, not only painted clipped
content. `home_heading` therefore restricts
window containment to fixed controls and the first Recent repository row;
compact GitHub action-in-row and name-priority checks remain, along with every
other heading, fixed-control and wide-layout assertion. Production is unchanged.

## Amendment — 2026-10-12: readable Apple Dark secondary text (#1239)

Apple Dark's readable secondary text uses the existing
`text_sub = text_label = text_muted = #98989f`. This opaque secondary-label
approximation uses iOS `secondaryLabel` (`#ebebf5` at 60% on `#1c1c1e`,
rounded to `#98989f`), rather than assigning systemGray or tertiary-label colours to
readable captions. It covers counts, timestamps, labels and Unstaged/Staged
section headings through the existing theme consumers; no new control, role,
schema, font size, geometry, Git colour or accent is introduced.

WCAG contrast calculated from source sRGB relative luminance (not screenshot
pixels) is:

| Text token/value | Panel `#161618` | Base `#1c1c1e` |
|---|---:|---:|
| Previous `text_muted = #636366` | 3.018:1 | 2.842:1 |
| Previous `text_label = #7c7c80` | 4.347:1 | 4.093:1 |
| New readable secondary `#98989f` | 6.305:1 | 5.937:1 |

Tertiary-label alpha (about 25%) belongs to disabled/placeholder content, not
readable secondary strings. The current Theme has no dedicated disabled or
placeholder token: gpui-kit controls inherit `muted_foreground` from
`text_muted`, so their disabled text also becomes somewhat brighter.
The existing disabled opacity contract is unchanged; no new disabled contrast
requirement or component patch is added. This is an explicit minimal-cutover
decision, not preservation of the old disabled painted RGB.

Apple Light is unchanged: `text_sub = text_label = #606060`,
`text_muted = #666666`, panel `#f7f7f7` and base `#ffffff`.
The unit regression computes WCAG sRGB contrast for both Apple presets,
requires all three readable secondary tokens to reach 4.5:1 on panel/base,
and pins Light's existing values. Existing native `hairline`,
`settings_quiet`, `settings_radio_dot`, `sidebar_panes` and
`commit_panel_viewport` scenarios remain unchanged.
Real-window comparison remains Tier B: Apple Dark Commit Panel open
(Unstaged/Staged headers and counts), Inspector (labels and metadata), and
PR list (captions and timestamps), retaining readable/disabled hierarchy.

Review correction: readable text is not a control-boundary role. Apple Dark
defines the separate neutral `INPUT_BORDER = #67676a`, with source-sRGB
contrast 3.205:1 on panel and 3.018:1 on base, quieter than secondary labels.
The proposed `#4a4a4d` would reach only 2.046:1 / 1.927:1, so it is not used.
The kit bridge maps Dark `colors.input` and the Commit Panel composer frame
to this neutral; `colors.border` derives from it at 85% alpha, preserving
quieter generic table/track borders without imposing the identifying-control
3:1 rule on decorative lines. Apple Light mappings remain unchanged.

Where `text_muted == text_sub`, the resting scrollbar thumb uses muted text
at 65% alpha while hover/drag keeps opaque secondary text, and the active line
number uses `text_main` instead of the identical secondary token. Unequal-role
themes keep their existing mappings. Unit coverage pins both Apple input
boundaries and checks equal-role hover/active distinction across every preset.

## Amendment — 2026-10-12: Apple Dark elevation ladder (#1274)

Floating surfaces are lighter than window chrome in Dark appearance. The
previous palette, modal shell and toast painted `panel = #161618`, while
menus and the component popover/tooltip bridge painted `modal = #2c2c2e`.
They therefore disagreed about elevation. Reuse the existing `modal` field
as the single elevated fill: `ELEVATED = #2a2a2c`; do not add another
custom-theme schema field or alias.

| Layer | Apple Dark source sRGB | Consumers |
|---|---|---|
| Base | `#101012` | Graph/diff content and sidebar base |
| Panel | `#161618` | Inspector and bottom-panel body |
| Chrome | `#1c1c1e` | Unified workspace/Home frame, repository tab strip, status bar, bottom-panel tabs |
| Elevated | `#2a2a2c` | Command palette, dialog/modal cards, menus, popovers, toasts, tooltips |

The ordering is strict relative luminance: **base < panel < chrome <
elevated**. Chrome boundaries use the existing `surface = #2c2c2e` as
literal one-logical-pixel hairlines. Elevated boundaries use opaque neutral
`ELEVATED_BORDER = #414144`: 1.673:1 against chrome and 1.408:1 against
elevated, exceeding the 1.3:1 separation target. Status glyphs and text keep
their existing semantic accent colours; the floating card outline is neutral.

The Apple-only resolution methods preserve every prior caller-specific
fill/border for Apple Light, other presets and custom themes, including themes
derived from Apple Dark with explicit overrides. The component bridge uses
the same elevated fill for popovers/menus/tooltips and the same chrome fill
for its title bar. Kagi-owned floating outlines resolve `ELEVATED_BORDER`;
the pinned library retains its stronger shared neutral border. Keyboard/AX
handlers, single-modal ownership and plan/confirmation semantics are unchanged.

The #1239 WCAG relative-luminance helper is reused, not duplicated. Opaque
secondary labels `#98989f` have 6.631:1 on the new base, 6.305:1 on panel,
5.937:1 on chrome and 4.998:1 on elevated. Main white text reaches 14.325:1
on elevated. ADR-0126's yellow accent `#ffd600` remains 13.459:1 on base and
10.144:1 on elevated. Resting and active line numbers retain their
secondary/white distinction. Only Apple Dark's resting scrollbar alpha
increases from 65% to 80%; composited on base it is `#7d7d83`, 4.646:1,
with opaque secondary hover/drag remaining distinct. Other themes' alpha
and identifying input boundary (`#67676a` on Dark) remain unchanged.

Review correction: preserve #1239's generic component border
`#67676a` at 85% alpha, rather than weakening Markdown tables, Kbd outlines
and scrollbar tracks by globally assigning `ELEVATED_BORDER`. The library
has no separate popover-border token; its popup/Tooltip borders retain this
stronger neutral. Kagi-owned cards and menus set their floating boundaries
individually.

Inside elevated cards, Settings/palette/platform-menu separators use
`ELEVATED_BORDER`, not the nearly identical `surface = #2c2c2e`.
The Settings native oracle pins `#414144` and requires at least 16 channel
steps between line and fill, well outside both ±2 RGB tolerance bands:
missing or thickened lines cannot masquerade as a 1px separator.
Clone/worktree/remote repository chips retain their `surface` fill, with a
Dark-only `ELEVATED_BORDER` outline; secondary labels therefore keep 4.863:1
contrast instead of becoming unreadable on an opaque border-coloured fill.
Remote directory selection and hover use `selected = #3a3a3c` in Dark.
Trust Repo, the co-author menu and full-hash Tooltip also use the single
elevated fill with identifying boundaries; their Light styling is unchanged.

Native Tier A `dark_elevation_palette`, `dark_elevation_modal`,
`dark_elevation_menu`, `dark_elevation_popover`, `dark_elevation_toast`
sample real Metal-rendered interior pixels against literal `#2a2a2c`.
Palette/modal/toast Light fills are pinned to `#f7f7f7`, and Light menus/
overflow remain `#ffffff`. `dark_elevation_chrome` pins chrome fills and
one-pixel boundary pixels. These are static-preset paint oracles, not a
claim of native vibrancy/material, focus, VoiceOver or live-input parity.

The existing `settings_quiet` oracle now pins the Dark floating fill and
neutral frame boundary while preserving its one-pixel separator/adjacent-pixel
checks and every Light expected RGB; it does not mistake Settings for panel
chrome. All six elevation scenarios and the neighboring Settings, radio,
sidebar, pane-hairline, primary-button, system-accent and Commit Panel viewport
scenarios passed the scoped native runner. Palette/modal Before runs observed
`#161618` where the new fill oracle requires `#2a2a2c`.
Review Tier A also passes `remote_browse_dark_elevation`: real native row
pixels pin accepted selection and pointer hover to `#3a3a3c`, idle rows to
`#2a2a2c`, and restored idle fill after pointer exit. The tightened Settings
Before run observed `[44, 44, 46, 255]` instead of separator `#414144`;
the corrected separators pass the full Light/Dark/zoom matrix.

### AppKit reference measurement and limitations

On this Mac (macOS 26.6 / 25G72), explicit `NSAppearance.darkAqua`,
Reduce Transparency and Increase Contrast both off, an ordered-out offscreen
window with forced active/inactive `NSVisualEffectView` state was rendered
with `cacheDisplay` and converted through an 8-bit sRGB CGContext.
The central-third histogram contained 28,329 uniformly opaque pixels per
capture. Native `.popover` / `.menu` material fallback samples were:

| Material | Active within-window | Active behind-window | Inactive |
|---|---|---|---|
| Popover | `#232323` | `#4b4b4b` | `#282828` |
| Menu | `#262626` | `#424242` | `#282828` |

Dark `windowBackgroundColor` sampled `#1e1e1e`. Varying the offscreen
backdrop black/`#1e1e1e`/white did not change these software fallback captures.
Evidence: `/tmp/kagi-1274-appkit-reference.0Nduwv/measurements.json` and
`method-and-findings.txt`, with native material PNGs.

This is **not a measurement of a live NSPopover or tracking NSMenu**:
macOS 26's realized popover glass produced a nonrepresentative white
`cacheDisplay` centre; a nontracking menu backing view remained transparent.
Live menu tracking was not started because it captures pointer events.
`#2a2a2c` is therefore a deliberately opaque static elevated-family
approximation, informed by the shared inactive `#282828` material sample,
not an exact native material match. No user setting, pointer or foreground
was changed. See Apple's [NSVisualEffectView](https://developer.apple.com/documentation/appkit/nsvisualeffectview)
and its [popover](https://developer.apple.com/documentation/appkit/nsvisualeffectview/material-swift.enum/popover) /
[menu](https://developer.apple.com/documentation/appkit/nsvisualeffectview/material-swift.enum/menu) materials.


## Amendment — 2026-10-12: Git-first narrow-toolbar priority (#1316)

This dense application-command surface reuses the existing measured fit cache,
header buttons and gpui-kit overflow menu; it adds no control or operation path.
The reference is Finder's secondary-toolbar overflow, adapted to Kagi's Git GUI
role: repository commands take priority over preferences and inactive navigation.

On shrinking, collapse Update → Settings → non-selected Analyze, Editor, PRs,
Graph → Terminal → Redo → Undo → Pop → Stash → Branch → Push → Pull → selected
workspace mode. Absent controls and the selected mode are skipped in the early
mode tier. Widening recomputes the same priority from cached natural widths,
restoring controls in reverse. This supersedes #1210's repository-first order.
When only the selected workspace mode remains visible (or no workspace item
remains), icon-mode fitting drops its 100-reference-pixel symmetric-column
floor while retaining actual selected-mode/overflow width and gaps. This
releases width for Git commands in production Graph mode, where Graph stays
selected until after Pull. A left-only 96-reference-pixel minimum budgets
Refresh, a 40-reference-pixel truncating repository/branch summary and the
drag spacer, so the right column can yield without making identity unreadable.
Wide windows retain equal flex columns; narrow windows may use asymmetric
minimums. Caption-mode measured identity/workspace minimums are unchanged.

Overflow menu order, Button/MenuItem roles, localized names, unavailable reasons,
action-bound shortcuts, keyboard focus and dispatch remain the #1252/#1217
contract. Hidden modes stay reachable through the existing keyboard shortcuts,
View menu and command palette as well as ». Rest/hover/pressed/focus-visible,
selected and disabled states retain their existing owners; no new loading,
error or safety lifecycle state is introduced.

Tier A `toolbar_overflow_priority` uses native width reductions and reverse
expansion with an Update offer, all four modes and Graph selected, across EN/JA,
100%/167% and Apple Light/Dark. Pure fit tests cover production's Graph, PRs
and Editor selected indices plus the no-selected-mode Takeover sentinel
(`usize::MAX`); Analyze never supplies selected index 11. Both presentation
modes run, with a production Graph mask proving selected-only width release.
The native oracle requires Pull/Push still visible below the old 100px-floor
fit threshold while Graph is the sole visible workspace item. Existing toolbar
fit, overflow, accessibility and workspace-mode scenarios retain their contracts.
Tier B target: inspect the narrow toolbar, readable repository/branch summary
and » in EN/JA at both zooms and Apple themes; verify hidden-mode shortcuts and
palette activation in the real application. Native bounds are not VoiceOver or
real-window visual evidence.

Regression evidence: the unchanged native priority oracle failed before the
policy change at Apple Light / EN / 100% / width 752: hidden mask `8320`
(Update + Terminal), expected `12288` (Update + Settings). After the cutover
it passes all eight language/theme/zoom cells, including final Pull/Push and
selected-Graph stages and reverse restoration. Existing `toolbar_overflow`
retains its original menu-membership/shortcut/AX/focus assertions at 940px,
then uses an explicitly constrained 480px viewport for Undo-reason, Terminal
focus and Branch activation: these commands no longer necessarily overflow at
940px under the new policy. Native resize requests use whole logical pixels,
matching AppKit's rounding rather than waiting for an impossible fractional
viewport at 167% zoom.

Review correction: the selected-only release is exercised with production
`present = 0x3fff`, selected Graph (`8`), not a synthetic no-Graph Git toolbar.
Removing both side reservations exposed a real identity-readability failure;
the left-only minimum above preserves the existing native >30px identity oracle.
The right floor still releases genuine space, and the native matrix requires
Pull/Push visibility below the old symmetric-100px fit threshold in every cell.


Tiny-window production review: native 700×500 / EN / Icon Only / 100% recorded
Terminal x=483.5..512 intersecting selected Graph x=495.5..524, matching the
reported Tier B overlap. Two coverage gaps had concealed it: the old matrix
began at 940px, and a feature-only `measure_control` wrapper around the Git group
changed its flex sizing. The group now records its own bounds with an absolute
inside probe and no test-only wrapper.

Natural-width canvases measure inside the fixed 2px focus border on each side.
Caption fitting already added those 4px; icon-only fitting must include them too.
Hidden command and Update probes carry the same border as visible controls so
style changes preserve one natural-width convention. The right column retains
its actual measured workspace/overflow minimum in both presentations; icon-only
repository identity explicitly shrinks to its existing readable minimum.

The 700×500 long-name matrix checks all visible outer control bounds (including
the fixed ring) and the actual Git group against every workspace control in both
presentations, EN/JA and 100%/167%. The tiny-height leg is a geometry oracle;
the existing 660px legs retain every operation/menu activation assertion.
The verification skill is updated. Real-window comparison remains Tier B.

Historical unified-frame fixture: the selected-only pure fit fixture subtracted
macOS's 68px traffic-light exclusion from its non-macOS available width. #1462
supersedes that reservation: the separate toolbar has no traffic-light exclusion.

## Amendment — 2026-10-13: restored titlebar document tabs (#1462)

**Owner decision:** return to two rows in every theme, for repository workspaces
and Home alike. This supersedes #1179's unified frame and single-document hiding;
it is an explicit layout reversal, not a theme-specific exception.

The main repository/Home window's first row starts at y=0 and is a fixed
36-logical-px titlebar, independent of shared density and UI zoom (#1500).
Its native macOS traffic lights retain their GPUI owner and the pre-#1179
(9,9) logical-px position, centred at every zoom, with an unscaled 80px exclusion
in this row only. Repository and Home document tabs are always visible, including
a single repository or Home-only document. The titlebar owns trailing + and tab
overflow; Home-only windows without a remote repository omit the inert +.
Home-only windows omit the inert × because there is no document behind Home.
The old #1180 40px strip geometry is superseded by this 36px titlebar, while its
always-visible single-document policy is restored.
Settings retains its independent 46-logical-px titlebar and #1198's native-light
position (9,16); the main-window reversal does not change Settings geometry.

**#1500 zoom alignment amendment (2026-10-14):** tab labels use the existing
14-reference-px small-text role capped at 14 logical px, so text fits the fixed
native chrome at 167%. Toolbar heights, typography and zoom are unchanged.
The titlebar overflow trigger also caps its height at 24 logical px, retaining
its shared keyboard/menu owner without changing toolbar overflow geometry.
Editor bottom-panel fitting subtracts the fixed 36-logical-px titlebar before
converting the remaining body budget to reference units; only the status bar
and toolbar contributions scale.
The `titlebar_tabs` matrix changes zoom on each actual macOS window through
100% → 167% → 100%, reads all three AppKit `standardWindowButton:` bounds
converted to the content view, and compares their centres with the titlebar
centre within 2 logical px. It also retains unscaled light-size/exclusion,
tab close/+, blank-only drag targets, overflow and resize assertions.

The second row is the toolbar: Icon Only remains 52 reference px (46.8 logical
px at 100%); Icon and Text remains 60 reference px (54 logical px at 100%).
Neither macOS traffic-light exclusion nor + participates in toolbar layout or
fitting. Preserve repository/branch identity, measured natural control widths,
centered wide-window actions, narrow overflow and #1316's Git-first collapse
priority; this decision changes row ownership, not action order or dispatch.

Only actual blank spacers own window gestures. macOS retains
`app_owns_titlebar_drag: true`; a blank-spacer left press/pressed move delegates
to `window.start_window_move()`, and its double-click delegates to
`window.titlebar_double_click()` for the person's zoom/minimize/none preference.
Windows retains native Drag hit testing on those same blank-only targets.
No tab, +, overflow trigger, toolbar button or their wrapping container is a
drag/double-click target.

Retain session-keyed `TabList`/`TabPanel` accessibility, one roving Tab stop,
manual keyboard activation, active/focused tab reveal, natural tab widths and
constrained horizontal overflow, neutral selected/inactive fills, localized
close/chooser names, dirty-close guards and existing native window titles.
Cmd+T/Cmd+W and File → New/Close Tab keep their command owners. Closing from two
documents to one must not hide the remaining tab or release its focus merely
because the count changed; #1179's hidden-strip focus and ordinary-content
TabPanel expectations are historical. Closing the final document still releases
focus through the existing owner, and keyed close/reindex behavior remains.

Overflow measures the outer titlebar's remaining width, excluding native lights,
the available + and the 8-reference-px blank-spacer minimum. Only while overflow
is needed does the tab group/viewport grow to that budget; the spacer stays at
its minimum. Widening the same window retires » and returns to intrinsic tab
widths without opening/closing documents or changing their cache signature.
Reservations snap the native-light exclusion, + and spacer widths separately to
GPUI's device-pixel grid (`round(width × scale_factor) / scale_factor`). Natural content
uses the already-snapped item span, without an extra pixel or integer ceiling;
contents that exactly fit the measured budget must not create ».
Chrome hairline oracles sample toolbar fill below the titlebar and body fill
below the toolbar/Home frame, matching the restored row order.

Native desktop `commit_panel_viewport` measures **404.5 logical px** for the file
list at 1392×883@100% after #1500: #1179's historical 440.5px minus the fixed
36px titlebar frame, including its literal 1px separator.
Fixed composer (257px), Generate (21.5px), Commit
(25px), desktop panel width (324px) and small-body AutoGrow contracts are unchanged.
At 940×560/167%, the added titlebar exposes the previous zero minimum on the
compound message group: its body/action row could extend below the group's
allocation and overpaint Commit. The group now reserves the existing Medium
subject, gap and body/action-row minimum together; the footer reserves its
existing padding, gaps, Small Commit and optional status/warning lines. With the
#1463/#1467 compact list integration, each file viewport reserves one scaled
22-reference-px row (at least 20 logical px) plus its 4px focus/border allowance;
file lists yield remaining height before the composer on very short windows.
AutoGrow measurement, desktop geometry and focus owners stay unchanged. Native
integration still measures 408px on desktop and keeps the composer/actions
contained at 940×560/167%; density and viewport oracles are not weakened.
The Home virtual-list overdraw rationale above remains authoritative: check
fixed chrome and the first Recent row against the window, and compact GitHub
actions/names against their row; do not require partially mounted virtual rows
below the fold to fit inside the window.


## Amendment — 2026-10-13: Finder-style on-accent labels (#1468)

Apple Light and Apple Dark's active list selection uses `ui_accent()` as
an opaque fill, except Graph commit/WIP rows (**#1485**, 2026-10-14).
Apple Graph uses the Graph-only `selection_wash(active)` token: 30% accent over
the default zebra stripe and lane tint, with normal text, badges and ref chips.
The existing System/Theme accent-source choice remains authoritative;
Dark's Theme default still uses its yellow accent. Non-key Graph selection
retains the independent opaque neutral `inactive_selected` token.
Selection identity, focus ownership, input behavior and row geometry do not change.
Other theme presets retain `selected_for_window(active)` unchanged; the wash
does not replace their selection identity. WIP diffstat keeps its ordinary
added/deleted colours directly, without an unused selected-foreground override.

**#1497 (2026-10-14):** Unselected Apple Graph WIP hover composites a lighter
15% `ui_accent()` wash over the same base/lane tint. Selected rows keep their
30% wash even under the pointer; hover does not change normal text, chip or
added/deleted diffstat colours. Non-Apple presets retain their existing hover
and selected tokens.

`Theme::on_accent` and the existing primary-button bridge share one
`on_accent_foreground` derivation for Apple accents: prefer white when its
contrast is at least **3.0:1**, otherwise black. **Owner decision: Finder
準拠で WCAG AA 4.5 ではなく 3.0 を採用** for these accent-filled labels.
System blue `#007aff` / `#0a84ff` therefore remains white, rather than changing
the exact accent fill or substituting black just to satisfy the previous AA
oracle. Light yellow, green and orange require black. This does not claim
WCAG AA compliance for normal-sized white accent text. Other presets and
custom themes retain their existing contrast-preserving on-accent choices.
Resolved Apple RGB/foreground pairs are cached, with no row-time native query
or repeated luminance calculation for an unchanged accent.

`selection_foreground(active)` and `selection_secondary_foreground(active)`
pair the foreground with the existing selected fill resolver. Apple key
secondary labels use 70% of the chosen on-accent foreground; non-key labels
return their normal theme roles. Actual selected-row text/glyphs and unfilled
badges switch together in navigation, files, PR and shared history rows.
Graph commit/WIP rows and their ref chips instead keep their normal semantic
foregrounds: the translucent wash is not an on-accent surface. Independently
opaque status chips retain their own fills and contrast labels. Primary accent
buttons reuse the on-accent bridge; menu/list highlights remain opaque selection.

**Semantic foreground guard:** `selection_uses_on_accent(selected, active)`
is the predicate for opaque row/menu foreground overrides. Graph rows do not
invoke it for their wash and never replace semantic status/check/diffstat colors
or muted labels. Only selected key-window Apple opaque rows replace those roles.
Inactive and non-Apple selections keep each original role. PR/Issue sidebar
cards apply the guard to their explicit child labels; Graph WIP chips do not.
Drag-over feedback uses the separate neutral segment/tint fill and retains its
ordinary foreground. Toolbar hover uses `surface`, distinct from current-mode
selection. Home pane and Conflict Preview/Edit switches are segments, not rows.

**Screenshot review correction:** selected segmented controls are not active
list rows. `segment_selected_bg()` therefore supplies the existing Apple
neutral grey (other presets keep their selected tint), independent of window
activation, with normal selected text/glyphs. Sidebar Graph/PRs/Issues,
toolbar modes, Inspector Path/Tree, Commit Panel List/Tree, bottom tabs,
editor toggles and filter chips use this separate role, including on hover.
Opaque accent + on-accent text is limited to active rows, accent-filled
primary buttons and menu highlights. Apple document tabs retain their existing
neutral document contrast. Structural expanded fills and text-selection
overlays remain separate. Radio/checkbox indicator glyphs keep the existing
4.5:1 generic-input foreground. Within the component bridge, Finder's
text-label policy changes only the filled-button foreground tokens.

Regression-first native `selection_on_accent` fails the previous Light fill
at painted `#d9ebff` versus required `#007aff`. It checks exact active/inactive
Metal fills, renderer-inherited and painted summary-label foregrounds,
retained selection/geometry, primary-button pixels and all-preset contrast.
Its segment leg checks actual sidebar and Commit Panel fill pixels are neutral,
not accent, and their labels remain ordinary in key and non-key windows.
PR-card and menu-highlight legs additionally inspect native fill and actual
child-label foregrounds, including muted/semantic retention outside the guard.
Enabled custom-menu hover and keyboard highlights share the selected fill and
foreground pair; shortcut text inherits 70% on-accent only when highlighted.
Shortcut labels retain element identity so GPUI resolves their group-hover
foreground during text layout, not only paint-time style refinement.
Disabled actions keep their existing subdued/danger presentation.
Existing activation and selected-hover oracles remain alongside it.
The Graph legs in `selection_on_accent` and `system_accent_chrome` now assert
the 30% accent composite instead of opaque accent; graph selection-bar pixels
remain opaque accent. `graph_selection_wash` covers Apple Light blue and
Apple Dark yellow at 100%/167%, normal primary ink and ≥4.5:1 contrast.
The opaque sidebar/file/menu/button oracles retain #1468's Finder policy.
Tier B compares the plain app before/after; native test recorders alone are
not evidence of Finder's material or AX delivery.

The integration with #1471/#1472 retains their compact file-row geometry,
13-reference-px file and diffstat typography, and inset Stage/Unstage focus
frames; the selected/key/Apple predicate changes only the foreground role.
PR hover keeps #1476's selected-row fill, while #1473's document strip remains
in the native titlebar without a second body strip. Graph Copy ownership from
#1461 is unchanged.
The PR hover oracle scrolls the live list without changing its focused PR
before sampling the selected row and its neighbor. At the small high-zoom
viewport, an offscreen neighbor otherwise retains retired recorder bounds
matching the selected row; distinct live bounds now guard the unchanged
fill/identity assertions.

## Amendment — 2026-10-15: element-owned hover tooltips (#1591)

Kagi controls register hover tooltips through
`kagi_ui_core::tooltip::HoverTooltip`: terminal
`hover_tooltip(stable_id, text)` for text and
`hover_tooltip_builder(stable_id, ...)` for custom views. Configure the control
before this terminal call, which returns a transparent Element rather than
the original component. It forwards the actual control's layout and paint
without inserting children that would change Kit icon-button sizing.
This boundary delegates delay and unmount ownership to GPUI's element-owned
`StatefulInteractiveElement::tooltip` registration and adds keyed geometry
and window-activation invalidation.
gpui-kit `Tooltip` still renders the text/custom content and theme colors,
but this does not preserve the managed overlay's complete presentation.
The roughly fourteen formerly Kit-managed registrations now use GPUI's
cursor-relative placement and timing instead of Kit overlay animation and
its grace when moving between adjacent buttons. This behavior is closer to
standard macOS tooltips; it is not an AppKit tooltip implementation.
Do not use component-inherent `.tooltip(text)` / `.tooltip_builder(...)`
to install a Root-owned managed overlay.

The failure mechanism is an ownership mismatch: a managed overlay can outlive
its trigger when unmounting or moving the trigger removes the
`on_hover(false)` callback that would dismiss it. Kagi's pinned baseline
reproduced this natively: scoped `KAGI_GUI_E2E_ONLY=toast_tooltip_lifetime`
failed at `tests/recovery/toast_dismiss.rs:71:5` with
`En/remove: removed trigger: tooltip must be hidden`. Ordinary hover/leave
and the preceding rerender case succeeded; removal without a pointer event
left the measured real tooltip painted. Baseline artifacts are preserved at
`/tmp/kagi-hig-audit-20261008/sticky-tooltip-tierb/before-fail`.
After migration, the native run scoped to
`KAGI_GUI_E2E_ONLY='toast_,toolbar_overflow,settings_groups'` passed all eight
scenarios: `toast_dismiss`, `toast_tooltip_lifetime`, `toast_tooltip_scroll`,
`toast_stack_budget`, `toast_live_status`, `settings_groups`,
`toolbar_overflow` and `toolbar_overflow_priority`. Lifetime coverage exercises
EN/JA ordinary leave, rerender, unmount, replacement/movement, exit timer and
pending-show removal. Scroll coverage uses real wheel input within the 30%
toast budget, then actual AX Collapse removes the older hovered close trigger.
These are native Tier A results for the geometry-aware boundary and migrated
Kagi-owned registrations. Tier B is **pending: screen locked**, deferred
with owner authorization; no foreground interaction or screenshot is claimed.

Element-owned registration avoids handing dismissal to a longer-lived Root
overlay. The shared helper also observes window activation: deactivation
permanently cancels that tooltip content, and a fresh hover rebuilds it.
It also reads activation when the delayed content is created, so deactivation
during the show delay cannot create visible help in an already-inactive window.
The review regression failed before that guard with two content renders
instead of zero. The pure cancellation predicate runs under ordinary `cfg(test)`
in workspace CI; TestWindow additionally exercises delayed creation and recovery.
Hidden native positive/lifecycle cases use a `gui-e2e`-only, window-scoped
active-input override while running the same production cancellation code.
The inactive native assertion and all TestWindow activation tests use no override.
Official-main timer/unmount behavior below remains a source-level inference,
distinct from the observed failure on Kagi's pinned dependency.

The pinned GPUI element-owned path also retains a `source_bounds` snapshot;
it is not sufficient for movement without a pointer event. Before adding the
geometry guard,
`cargo test -p kagi-ui-core --features gui-e2e,gpui/test-support tooltip::tests`
passed the deactivation test but failed
`moving_a_live_trigger_cancels_help_without_mouse_input`: moving the same
live trigger from x=0 to x=200 produced render count 4 instead of 2.
The transparent boundary now records keyed current clipped bounds and cancels
help once those bounds no longer contain the pointer. Small animation shifts
that leave the pointer inside retain help. This keeps GPUI's delay/unmount owner and
the existing activation guard rather than introducing a managed Root overlay.
After the guard and delayed-activation review, the `tooltip::` filter with
`-- --nocapture` passes three TestWindow tests plus two ordinary cancellation
tests, including no resurrection after moving back/reactivating and fresh-hover
recovery. The eight-scenario run separately validates native rendering;
hidden-window positive cases substitute only activation input as described above.

The registration rule also covers Kagi-owned direct explanatory
`.tooltip(builder)` calls, including subjects, sidebar and toolbar help:
their source-bounds lifetime needs the same terminal boundary. Preserve
their content builders. Interactive `hoverable_tooltip` worktree cards remain
a distinct ownership path, not ordinary explanatory tooltips.
`check-hover-tooltip` bans direct explanatory `.tooltip`, `.tooltip_builder`
and `.tooltip_with_action` registration in application/UI-crate Rust sources,
excluding only the shared boundary. Samples preserve `hover_tooltip`,
`hover_tooltip_builder` and interactive `hoverable_tooltip`; the workflow matrix
and normal `check-all` run the gate.

Use official public APIs without dependency edits, fork commits or pin changes.
The owner-facing upstream proposal is recorded in `docs/decisions.md`: managed
overlays need trigger-lifetime, geometry and window-activation invalidation
independent of receiving a final hover-exit callback. Retire this Kagi
lifecycle boundary only after an official managed implementation provides
those guarantees and equivalent native regression coverage passes at Kagi's
adopted revision. Tooltip content, localized help, accessibility descriptions
and keyboard-opened inspection-card ownership remain unchanged.

### Official upstream inspection and unpublished proposal

Source inspection of official [gpui-kit](https://github.com/longbridge/gpui-kit)
`origin/main` at `50dac1cafb34a4d453c92715219c0458d9a4727b` found
hovered `GlobalElementId` ownership, pressed-trigger guards and click-removal
tests. These fixes already exist; this proposal does not claim otherwise.
In `crates/component/src/tooltip.rs`, `on_prepaint` stores bounds/identity,
while `on_hover(false)` is the dismissal path. In
`crates/component/src/root.rs`, `WindowState` has no mounted-trigger
reconciliation. In `crates/base/src/tooltip.rs`, `TooltipRequest` retains a
builder and bounds, and rendering retained content has no activation guard.
**[INFERENCE]** Non-click trigger removal, including removal during the
show delay, can therefore leave pending or visible managed tooltip state
alive. This is source-level analysis, not an upstream runtime reproduction.

**Draft upstream reproduction (not published or runtime-tested):**

1. Hover a Button with its managed tooltip until the tooltip is visible.
2. Remove that Button through a timer, asynchronous completion or conditional
   parent render, without pressing/clicking the trigger.
3. Move the pointer away. Expected: the tooltip is hidden when its trigger
   disappears. Predicted actual result from source inspection: the retained
   overlay remains because no final hover-exit callback runs.
4. Repeat with removal during the tooltip's show delay. Expected: cancel the
   pending show; it must not display after trigger removal.

**Proposal:** retain the existing owner-aware switching and press guards,
but add a trigger lease/frame-liveness check and reconcile current clipped
bounds/hit testing. Cancel pending show and clear visible state on unmount
or window deactivation; invalidate hover when geometry no longer contains
the pointer. Add lifecycle tests for timer/async removal before and after
show, movement/scroll/resize, deactivation/reactivation and fresh-hover
recovery. No upstream issue or PR has been published for this proposal.

The Kagi boundary covers Kagi-owned registrations, not dependency-internal
controls: gpui-kit's Input Find controls still install managed tooltips and
provide no public cleanup hook to Kagi. Their resolution belongs to the
official managed implementation, not a fork or a private-API workaround.

## Amendment — 2026-10-17: responsive confirmation actions (#1646)

At Japanese / 167% / 1000×700, Sync's armed caption consumed the entire
single-row footer and hid Cancel. The shared footer now reserves its height
and wraps in reverse line order: confirmation above Cancel when they cannot
fit beside each other. Construction and keyboard order stay Cancel first.
This follows the stacked-action placement in
[Apple's Alerts guidance](https://developer.apple.com/design/human-interface-guidelines/alerts)
without changing Kagi's safe Return/Escape policy or its two-stage gate.

Ordinary action captions retain their full text and native Button, using its
existing child API and the shared explicit accessibility-name helper. They
fit the available width and grow above the ordinary S-height minimum when
text wraps. Existing bounded identity captions keep their middle ellipsis,
full description and tooltip. No dependency fork or new interactive control
is required. Hover, pressed, focus-visible and unavailable semantics remain
the existing ones; only action layout changes.

Native confirmation probes forward the actual Button layout through the
existing measured-leaf element. An auto-width test-only wrapper prevented
percentage width constraints from resolving as in the production footer;
it must not turn a passing real layout into a different test layout.
Containment/nonoverlap, actual Cancel activation and unchanged repository
state are required in both confirmation stages, including expanded recovery.
Native geometry is not a substitute for real-window or human visual review.

## Amendment — 2026-10-26: one titlebar centre line (#1790)

The main window's traffic lights move from (9,9) to (9,11). Measured on a 2×
window, (9,9) put their centre at y≈15.75 in the 36px row while the document
tabs sit at y=18, so "centred" above was not true. (9,11) puts the lights, the
new sidebar toggle after them and the tabs on one centre line. Settings keeps
(9,16).
