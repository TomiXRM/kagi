# ADR-0133: Per-theme syntax colours

- Status: Accepted
- Date: 2026-07-26

## Context

User report: "whatever theme I pick the syntax highlighting is the same —
Apple Dark and Catppuccin Mocha look identical. I made distinct themes, I
want distinct colours."

Correct, and structural rather than cosmetic. kagi shows code through two
independent paths — the diff views (`diff_view::highlight_diff_rows*`) and
gpui-component's `CodeEditor` (Editor Workspace, Snapshot, Conflict editor)
— and **both** took gpui-component's bundled `HighlightTheme::default_dark()`
/ `default_light()`, selected by `Theme::dark` alone. kagi's `Theme` struct
had no syntax fields at all; the only trace of intent was prose in comments
("keyword pink #ff7ab2 …") that was never encoded as data.

Two consequences followed:

- every dark theme highlighted identically, as did every light one;
- the bundled palette defines no `variable`, `operator` or `punctuation`, so
  identifiers, operators and `{}` / `::` — most of any Rust file — fell back
  to plain foreground. A second user report ("`{}` and `::` were missing")
  was the same root cause seen from the other side.

The two paths had also drifted: the diff called `default_dark()` directly and
never saw even the five editor-surface overrides `sync_gpui_component_theme`
applied, so diff and editor disagreed about foreground.

## Decision

### `SyntaxPalette`: ten colours per theme, the rest derived

`Theme` gains `syntax: SyntaxPalette` — keyword, string, comment, type,
function, number, operator, punctuation, variable, attribute.

Ten, not gpui-component's ~42, because ten is the honest granularity: the
upstream palettes these are ported from distinguish roughly that many roles
and the rest are aliases (`boolean` is a `number`, `enum` is a `type`,
`comment.doc` is a `comment`). Ten keeps 11 themes reviewable; 42 would fill
the table with repeats.

Where an upstream theme deliberately gives a role no colour — Xcode colours
neither operators nor punctuation, and Dracula/Monokai/One Dark leave
punctuation plain — the entry is set to that theme's own `text_main`. Flat is
the design there; inventing a colour would misrepresent the theme.

Colours are ported from each palette's own source (Apple's
`.xccolortheme` plists, Catppuccin's style guide, Dracula's spec, VS Code's
bundled Monokai, One Dark Pro, Tokyo Night, Pinky Boo's published theme).
**`ibm-pc` is the exception and is marked as such in code**: no IBM PC syntax
theme exists, so the values are exact CGA/EGA hardware-palette entries with a
token mapping of our own choosing.

### Built as JSON, not field-by-field

`highlight_theme(&Theme)` renders a Zed-format theme document and
deserialises it. gpui-component's `ThemeStyle` keeps `color`/`font_style`
private, so JSON is the only public way to construct `SyntaxColors` — and it
avoids re-forking gpui-component, which this repo deliberately stopped doing
in the zed-main migration. Cost is one parse per theme switch.

A malformed literal falls back to the bundled preset rather than killing the
app mid-switch; a unit test asserts every theme actually builds (the
fallback is a safety net, not an accepted outcome).

### One highlight theme for both paths

`diff_view` now calls the same `theme::highlight_theme(theme::theme())` the
CodeEditor gets, so the two paths cannot drift again.

### Xcode themes retired

`xcode-dark` / `xcode-light` were byte-identical to `apple-dark` /
`apple-light` — the same Apple theme files — so they are removed and the
Apple themes carry Xcode's official code colours. `index_of` maps the retired
slugs onto their successors so an existing `settings.json` (or `KAGI_THEME`)
doesn't silently drop to the default; the theme-menu commands redirect too.

## Consequences

- Adding a theme now means adding ten more colours. The contrast test below
  makes that hard to get wrong.
- A `syntax_colours_are_legible_on_their_background` test holds every theme
  to 3.0:1 against its own background. It caught a real defect: Pinky Boo, a
  *light* theme derived from One Dark Pro, had inherited several of its
  ancestor's *dark* token colours — `string` measured 1.9:1, effectively
  invisible (user report: "text goes white"). Those are darkened here, hue and
  saturation preserved, with the upstream value recorded per line.
- Catppuccin Latte is exempted by name: it ships 2.3-3.0:1 upstream by its own
  design, and people choose it knowing how it looks. The exemption is a
  visible list, not a lowered threshold.
- 3.0 rather than WCAG AA's 4.5 is deliberate — several published palettes sit
  in the 3s, and the test is meant to catch "illegible", not "lower contrast
  than I would have chosen".
