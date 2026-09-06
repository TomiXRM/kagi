# ADR-0175: Flower Road lane-palette candidates

- Status: Accepted
- Date: 2026-09-07
- Touches: `crates/kagi-ui-core/src/theme_flower_road*.rs`,
  `crates/kagi-ui-core/src/theme.rs`
- Builds on: ADR-0104 (stable eight-colour swimlanes), ADR-0133 (theme registry)

## Context

Flower Road's original graph palette is deliberately soft: its flower colours
are used as supplied, which puts the eight lane strokes at 1.4–2.3:1 against
the theme's ivory background. It also has two closely related blue lanes and a
neutral silver lane. The result preserves the original flower-card feeling but
does not make a balanced categorical colour wheel comparable to Apple Dark.

The graph layout assigns a stable colour index modulo eight. Increasing that
number would alter the domain contract and every theme, although the requested
work is only a Flower Road visual exploration.

## Decision

Keep `Flower Road` unchanged and offer two registered variants in the existing
theme picker:

1. **Flower Road Bloom** uses eight equally spaced hue families: poppy, leaf
   green, iris, marigold, hydrangea, dahlia, chrysanthemum, and cornflower.
2. **Flower Road Vivid** uses the same wheel and ordering with higher
   saturation, for a categorical impact closer to Apple Dark.

Both variants use the existing Flower Road warm-white backgrounds, text,
syntax, diff, terminal, and semantic colours. Only `lane_hsl` differs. The
eight hues are stored in a three-step order around the wheel, so adjacent lane
indices differ by 135°; their sorted positions remain exactly 45° apart. Every lane meets
3.5:1 contrast against the ivory background.

## Consequences

- Users can compare the original soft palette with two balanced alternatives
  immediately through Settings or the command palette, without a new setting
  or rendering path.
- The `NUM_COLORS = 8` domain invariant and Apple/Dark theme behaviour remain
  unchanged.
- A unit test guards the new variants' even hue coverage and lane contrast.
- Each built-in theme has its own module; `theme.rs` now owns only the shared
  types, helpers, palettes, and registry.
