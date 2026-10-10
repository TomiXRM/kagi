# Custom themes

kagi ships built-in colour themes (Settings → Appearance, the command palette's
`Theme: …` entries, and View → Theme). You can add your own as JSON files.
Design record: [ADR-0220](adr/0220-json-user-themes.md).

## Where theme files live

| Situation | Folder |
|---|---|
| `KAGI_LOG_DIR` is set (non-empty) | `$KAGI_LOG_DIR/themes/` |
| Otherwise | `~/.kagi/themes/` |

This is the `themes` folder next to kagi's `settings.json`. kagi reads every
regular file in that folder whose extension is `.json` (any case). Sub-folders
are not searched. Files are read in filename order. The filename itself does not
matter otherwise: a theme is identified by its `slug`.

In **Settings → Appearance → Custom themes**, the resolved folder path is
selectable and copyable. **Open theme folder** creates the directory if missing,
then opens it with the OS file manager (Finder on macOS, `xdg-open` on Linux).
It does not create a theme file or write `settings.json`; failures appear as an
error toast. **Theme file guide** opens this page.

## Loading and reloading

- Themes are loaded once at startup, before kagi restores the theme you last
  picked, so a custom theme can be the one restored on launch (`KAGI_THEME=<slug>`
  still overrides the saved choice).
- After editing files, click **Reload Themes** in Settings → Appearance, or
  run it from the command palette (Japanese UI: 「テーマを再読み込み」).
  Loading and parsing run in the background; only the latest requested reload
  updates the UI if you click repeatedly. kagi does not watch the folder.
- Custom themes appear by their `name` in Settings, the command palette and
  View → Theme, sorted together with the built-in themes (case-insensitive,
  by `name`). Picking one saves only its `slug` to `settings.json` (key `theme`),
  exactly like a built-in theme.
- If the active custom theme is gone after a reload (file deleted, renamed slug,
  or now invalid), kagi switches to the default theme (Catppuccin Mocha) for the
  rest of the session but does **not** overwrite your saved choice. Restore the
  file and restart (or reload and pick it again) to get it back.

## File format

A theme file is one JSON object.

- `slug` — **required.** 1–64 characters from `a-z`, `0-9` and `-`, starting with a
  letter or digit. It is what `settings.json` and `KAGI_THEME` store. It must not
  equal a built-in slug (`catppuccin`, `catppuccin-latte`, `apple-dark`,
  `apple-light`, `color-vision`, `dracula`, `flower-road`, `ibm-pc`, `monokai`,
  `one-dark`, `one-light`, `pinky-boo`, `tokyo-night`) or a retired alias
  (`xcode-dark`, `xcode-light`, `flower-road-bloom`, `flower-road-vivid`).
- `name` — **required.** Non-empty display name shown in menus and Settings.
- `extends` — optional. The slug of a **built-in** theme. Every key you leave out
  is copied from that theme; the keys you write replace its values. A custom
  theme cannot extend another custom theme.
- Every other key is a token from the [token reference](#token-reference) below,
  spelled exactly as listed.
- **Without `extends`, every token is required** (all keys in the table,
  including `dark`, all ten `syntax` keys, `lane_hsl` and `term_selection`).
- With `extends`, any subset of tokens may be given. `syntax` may list only the
  code colours you want to change; the rest come from the parent.

Value types:

| Kind | JSON | Example |
|---|---|---|
| Colour | string `"#rrggbb"` (six hex digits, no alpha) | `"#1e1e2e"` |
| `dark` | boolean | `true` |
| `lane_hsl` | array of exactly 8 `[hue, saturation, lightness]` arrays, each number in `0`–`1` (hue is degrees ÷ 360) | `[[0.937, 1.0, 0.749], …]` |
| `avatar_sat`, `avatar_light` | number in `0`–`1` | `0.7` |
| `term_selection` | object with exactly `color` (`"#rrggbb"`) and `alpha` (integer `0`–`255`) | `{ "color": "#585b70", "alpha": 153 }` |
| `syntax` | object whose keys are the `syntax.*` tokens below, each a colour | `{ "keyword": "#cba6f7", … }` |

### Minimal example: change a few colours

`~/.kagi/themes/mocha-tokyo-accents.json` — Catppuccin Mocha with Tokyo Night's
blue accent, selection tint and purple accent:

<!-- example:extends -->
```json
{
  "slug": "mocha-tokyo-accents",
  "name": "Mocha (Tokyo accents)",
  "extends": "catppuccin",
  "color_branch": "#7aa2f7",
  "selection_tint": "#7aa2f7",
  "accent": "#bb9af7"
}
```

### Complete example: every token

`~/.kagi/themes/my-mocha.json` — a standalone copy of the default theme
(Catppuccin Mocha) with every token spelled out. Start from this to build a
theme from scratch:

<!-- example:complete -->
```json
{
  "slug": "my-mocha",
  "name": "My Mocha",
  "dark": true,
  "bg_base": "#1e1e2e",
  "bg_row_alt": "#1a1a2a",
  "surface": "#313244",
  "selected": "#45475a",
  "panel": "#181825",
  "sidebar": "#11111b",
  "modal": "#313244",
  "modal_overlay": "#000000",
  "text_main": "#cdd6f4",
  "text_sub": "#a6adc8",
  "text_muted": "#585b70",
  "text_label": "#6c7086",
  "link": "#89b4fa",
  "color_head": "#f38ba8",
  "color_branch": "#89b4fa",
  "color_remote": "#a6e3a1",
  "color_tag": "#fab387",
  "selection_tint": "#89b4fa",
  "color_success": "#a6e3a1",
  "color_warning": "#f9e2af",
  "color_blocker": "#f38ba8",
  "color_blocker_muted": "#8f5360",
  "diff_added_bg": "#1c3a2a",
  "diff_removed_bg": "#3a1c1c",
  "diff_hunk": "#89b4fa",
  "change_added": "#a6e3a1",
  "change_modified": "#f9e2af",
  "change_deleted": "#f38ba8",
  "change_renamed": "#89b4fa",
  "change_typechange": "#585b70",
  "change_dir": "#6c7086",
  "accent": "#cba6f7",
  "lane_hsl": [
    [0.937, 1.0, 0.749],
    [0.268, 0.546, 0.558],
    [0.619, 1.0, 0.770],
    [0.059, 1.0, 0.647],
    [0.477, 1.0, 0.421],
    [0.783, 1.0, 0.780],
    [0.129, 1.0, 0.437],
    [0.535, 1.0, 0.500]
  ],
  "avatar_sat": 0.70,
  "avatar_light": 0.60,
  "term_bg": "#1e1e2e",
  "term_fg": "#cdd6f4",
  "term_cursor": "#f5c2e7",
  "term_black": "#45475a",
  "term_red": "#f38ba8",
  "term_green": "#a6e3a1",
  "term_yellow": "#f9e2af",
  "term_blue": "#89b4fa",
  "term_magenta": "#cba6f7",
  "term_cyan": "#89dceb",
  "term_white": "#bac2de",
  "term_bright_black": "#585b70",
  "term_bright_red": "#f38ba8",
  "term_bright_green": "#a6e3a1",
  "term_bright_yellow": "#f9e2af",
  "term_bright_blue": "#89b4fa",
  "term_bright_magenta": "#cba6f7",
  "term_bright_cyan": "#89dceb",
  "term_bright_white": "#cdd6f4",
  "term_selection": { "color": "#585b70", "alpha": 153 },
  "syntax": {
    "keyword": "#cba6f7",
    "string": "#a6e3a1",
    "comment": "#9399b2",
    "type_name": "#f9e2af",
    "function": "#89b4fa",
    "number": "#fab387",
    "operator": "#89dceb",
    "punctuation": "#9399b2",
    "variable": "#eba0ac",
    "attribute": "#f9e2af"
  }
}
```

## Invalid files

Each file is checked on its own. A file is rejected — and only that file; other
themes and kagi itself keep working — when it:

- is not a single valid JSON object;
- has a key that is not `slug`, `name`, `extends` or a token below (also inside
  `syntax` and `term_selection`) — typos are errors, not ignored;
- misses `slug` or `name`, or (without `extends`) misses any token;
- has a value of the wrong type, a colour that is not `#rrggbb`, a `lane_hsl`
  that is not exactly 8 triples, a number outside `0`–`1`, or an `alpha`
  outside `0`–`255`;
- `extends` something that is not a built-in slug;
- uses a built-in slug, or a slug an earlier file (by filename order) already
  took.

kagi logs the reason and, once the window is up, shows one toast per rejected
file: “Couldn't load theme `<filename>`: `<reason>`”.

VS Code (or other editor) theme files are not read; convert them by mapping their
colours onto the tokens below.

## Button operation roles

Callers choose `ButtonRole` from the operation, never by matching an RGB value.
Themes may intentionally reuse colours (Color Vision uses blue for both branch
and success, orange for both remote and blocker); that cannot change hierarchy.
The existing `KagiButton` delegates geometry, focus, disabled and interaction
handling to the pinned gpui-component Button. No new control is introduced.

| Operation role | Theme token | Button presentation |
|---|---|---|
| `Primary` | `color_branch` | Filled Primary (for the main confirm/Commit action). |
| `SideCurrent` | `color_branch` | Filled Primary (Keep Current / Keep Directory). |
| `SideIncoming` | `color_remote` | Filled Info (Take Incoming / Keep File). |
| `Success` | `color_success` | Translucent tinted action (Stage / Save / Continue / Approve). |
| `Warning` | `color_warning` | Translucent tinted action (Unstage / Reset before arming / Request Changes). |
| `Danger` | `color_blocker` | Translucent tinted action (Abort / armed Reset / destructive modal confirm). |
| `Neutral` | `text_sub` | Ghost (Keep Both / paging / external raw-side action). |
| `NeutralTinted` | `text_sub` | Neutral tinted chip (editor navigation / external editor action). |

These are dense conflict/tool/action-row controls, keeping their current sizes;
the reference is Kagi's existing Apple-theme filled side choice versus tinted
Stage/Discard, not a new palette. Rest, hover, pressed, focus-visible and disabled
remain the native Button states; side choices have no selected/loading/error
state of their own. Existing gates, handlers and plan/confirm safety flow remain.

The bridge owns the complete filled Info family (`button_info`, foreground,
hover, active). Its rest fill is exactly `color_remote`, matching the Incoming
pane/marker (Apple Light `#34c759`). Its label and opaque interaction shades use
the same contrast-preserving derivation as Primary; upstream preset Info colours
cannot leak into side choices. Success/danger tints and generic info notices keep
their existing token meanings.

## Body links

`link` is a body-text foreground token mapped directly to gpui-component's
`colors.link` (Issue composer previews, Issue/PR descriptions and conversation
TextViews). It is not the filled `color_branch` accent. Built-ins target at least
4.5:1 over `bg_base`; Apple Light uses `#0066cc` (5.567:1 on white), while its
ref chips and Primary fill stay `#0088ff`. Already-readable dark palettes,
including Apple Dark, keep their prior link colour. Latte, One Light, Pinky Boo,
Flower Road and IBM PC also have a separately readable link shade.

Custom themes inherit `link` with `extends`, or set `"link": "#rrggbb"` explicitly.
Changing only `color_branch` no longer changes body links. A standalone complete
theme must supply `link`; choose it against your actual body background, since
explicit custom colours are not silently corrected. Underlining, activation,
selection tint, typography and geometry are unchanged. Reference: macOS body
links, not filled ref chips; required rest/selection states keep the existing
TextView behavior (no separate button/loading state).

## Token reference

Every token a theme defines. The middle column is the default theme's
(Catppuccin Mocha, slug `catppuccin`) value; the last column is where kagi
actually paints it. kagi also derives a few colours itself: filled buttons
(primary, incoming and warning) put `bg_base`, then `text_main`, then black/white on their
fill — whichever first reaches WCAG AA contrast; text drawn in the accent is
lightened or darkened via `accent_text_on` until it reads at 4.5:1; the text
selection wash is `selection_tint` at 30 % opacity.

<!-- theme-tokens:start -->
| Token | Catppuccin Mocha | Used for |
|---|---|---|
| `slug` | `catppuccin` | Stable id saved in `settings.json` (`theme`) and accepted by `KAGI_THEME`; logged at startup as `[kagi] theme: <slug> dark=<bool>`. |
| `name` | `Catppuccin Mocha` | Label in Settings, the command palette (`Theme: <name>`) and View → Theme; also the sort key of those lists. |
| `dark` | `true` | Dark/light mode for gpui-component widgets and its base colour preset, the code highlighter's appearance, ref-badge text (white vs `text_main`), section-panel tint, Markdown preview and Mermaid diagram rendering, and several dark-vs-light alpha choices (button hover, compact graph-lane tint, WIP chips, header and PR highlights). |
| `bg_base` | `#1e1e2e` | Window and list base background (gpui-component `background`, `list`, scrollbar track), code-editor background, context lines in diffs; preferred label colour on filled buttons. |
| `bg_row_alt` | `#1a1a2a` | Zebra stripe of odd commit/file rows, hover on conflict file rows, code-editor active-line highlight. |
| `surface` | `#313244` | Hover background of rows, dividers/borders between panes, neutral buttons and chips (gpui-component `muted`, `secondary`, `button`, list hover, title-bar border). |
| `selected` | `#45475a` | Selected row (commit list, conflict files, tabs), hover of toolbar buttons, border of popovers (command palette, menus). |
| `panel` | `#181825` | Even commit rows, detail/inspector panel, commit panel, command palette, menus and modal cards, PR pages, title bar. |
| `sidebar` | `#11111b` | Sidebars: PR and issue navigation, conflict dashboard (gpui-component `sidebar`). |
| `modal` | `#313244` | Context-menu background, gpui-component popovers (e.g. tooltips), ecosystem viewer modal card. |
| `modal_overlay` | `#000000` | Scrim behind full-screen modals (drawn with opacity; gpui-component `overlay`). |
| `text_main` | `#cdd6f4` | Primary text, input caret, code-editor foreground, enabled menu items; ref-badge and panel tint on light themes. |
| `text_sub` | `#a6adc8` | Secondary text: commit author/stat columns, header text, progress notes; scrollbar thumb hover; active line number. |
| `text_muted` | `#585b70` | Dimmed text and disabled menu items, divider/input borders, scrollbar thumb, line numbers, inline-code chip tint, unknown change-kind badge. |
| `text_label` | `#6c7086` | Field and section labels (Unstaged/Staged headers, modal input labels, conflict section titles, issue comment count). |
| `link` | `#89b4fa` | Body-link foreground in gpui-component TextView (`colors.link`): Issue composer preview and Issue/PR body/conversation links; independent of filled ref/Primary accent. |
| `color_head` | `#f38ba8` | HEAD branch badge in the graph and inspector, merge line of the activity chart, link values in the commit trailer table. |
| `color_branch` | `#89b4fa` | Local branch/worktree badges and the main accent: primary buttons, focus ring, checkbox, info notices, drag handles, the "current" side of conflicts, loading dots. |
| `color_remote` | `#a6e3a1` | Remote-branch badges, the "incoming" side and its buttons in the conflict views, info-style buttons. |
| `color_tag` | `#fab387` | Tag badges, icon of the create-tag and push-tag dialogs. |
| `selection_tint` | `#89b4fa` | Text selection in inputs and selectable text, selected lines in the unified and split diff views (at 30 % opacity). |
| `color_success` | `#a6e3a1` | Success text and buttons (Continue, resolved counts), activity-chart commit line, loading dots. |
| `color_warning` | `#f9e2af` | Warning text and warning buttons (needs-review, skip confirmation). |
| `color_blocker` | `#f38ba8` | Errors and destructive actions: danger buttons (Abort), unresolved-conflict status, blocker notes. |
| `color_blocker_muted` | `#8f5360` | Disabled menu items whose action is destructive. |
| `diff_added_bg` | `#1c3a2a` | Background of added lines in unified/split diffs and PR review snippets. |
| `diff_removed_bg` | `#3a1c1c` | Background of removed lines in diffs and PR review snippets; conflicted file rows in the commit panel. |
| `diff_hunk` | `#89b4fa` | Hunk header (`@@ … @@`) text in diffs and PR review snippets. |
| `change_added` | `#a6e3a1` | `A` change badge, added-line text and `+N` counts, green squares of the diffstat bar. |
| `change_modified` | `#f9e2af` | `M` change badge. |
| `change_deleted` | `#f38ba8` | `D` change badge, removed-line text and `-N` counts, red squares of the diffstat bar. |
| `change_renamed` | `#89b4fa` | `R` (renamed) and `C` (copied) change badges. |
| `change_typechange` | `#585b70` | `T` (type change) badge. |
| `change_dir` | `#6c7086` | Directory names in the changed-files trees (inspector, commit panel, editor, commit preview). |
| `accent` | `#cba6f7` | Cherry-pick button in the inspector, middle loading dot. |
| `lane_hsl` | `[0.937, 1.0, 0.749]`, … (8 triples) | Commit-graph lane colours (lines, nodes, ref pills in swimlane mode), WIP rows and the matching worktree tab, PR commit lanes; cycles every 8 lanes. |
| `avatar_sat` | `0.70` | Saturation of initial-letter avatar circles (hue comes from the author e-mail) in the graph and inspector. |
| `avatar_light` | `0.60` | Lightness of the same avatar circles. |
| `term_bg` | `#1e1e2e` | Integrated terminal background. |
| `term_fg` | `#cdd6f4` | Integrated terminal default text. |
| `term_cursor` | `#f5c2e7` | Integrated terminal cursor. |
| `term_black` | `#45475a` | Terminal ANSI colour 0 (black). |
| `term_red` | `#f38ba8` | Terminal ANSI colour 1 (red). |
| `term_green` | `#a6e3a1` | Terminal ANSI colour 2 (green). |
| `term_yellow` | `#f9e2af` | Terminal ANSI colour 3 (yellow). |
| `term_blue` | `#89b4fa` | Terminal ANSI colour 4 (blue). |
| `term_magenta` | `#cba6f7` | Terminal ANSI colour 5 (magenta). |
| `term_cyan` | `#89dceb` | Terminal ANSI colour 6 (cyan). |
| `term_white` | `#bac2de` | Terminal ANSI colour 7 (white). |
| `term_bright_black` | `#585b70` | Terminal ANSI colour 8 (bright black). |
| `term_bright_red` | `#f38ba8` | Terminal ANSI colour 9 (bright red). |
| `term_bright_green` | `#a6e3a1` | Terminal ANSI colour 10 (bright green). |
| `term_bright_yellow` | `#f9e2af` | Terminal ANSI colour 11 (bright yellow). |
| `term_bright_blue` | `#89b4fa` | Terminal ANSI colour 12 (bright blue). |
| `term_bright_magenta` | `#cba6f7` | Terminal ANSI colour 13 (bright magenta). |
| `term_bright_cyan` | `#89dceb` | Terminal ANSI colour 14 (bright cyan). |
| `term_bright_white` | `#cdd6f4` | Terminal ANSI colour 15 (bright white). |
| `term_selection` | `{ "color": "#585b70", "alpha": 153 }` | Terminal selection highlight; translucent so the selected text stays readable. |
| `syntax.keyword` | `#cba6f7` | Code keywords (`fn`, `let`, `if`, …), HTML-style tags, bold Markdown emphasis — in the code editor and diff highlighting. |
| `syntax.string` | `#a6e3a1` | String literals, regexes, literal text, link URLs. |
| `syntax.comment` | `#9399b2` | Comments and doc comments (italic), hints. |
| `syntax.type_name` | `#f9e2af` | Types, enums and enum variants. |
| `syntax.function` | `#89b4fa` | Functions, methods, constructors, titles, link text. |
| `syntax.number` | `#fab387` | Numbers, booleans, constants, string escapes. |
| `syntax.operator` | `#89dceb` | Operators and special punctuation. |
| `syntax.punctuation` | `#9399b2` | Brackets, delimiters, list markers. |
| `syntax.variable` | `#eba0ac` | Variables, properties, labels. |
| `syntax.attribute` | `#f9e2af` | Attributes (`#[derive(…)]`, decorators) and preprocessor directives. |
<!-- theme-tokens:end -->
