# gpui-terminal — Kagi-owned terminal

Kagi maintains this in-tree terminal frontend, selection/copy, scrollback
navigation and PTY integration. It derives from Leonard Seibold's
[`gpui-terminal` 0.1.0](https://github.com/zortax/gpui-terminal), published on
crates.io under MIT OR Apache-2.0. The package name and directory retain that
origin; this is not an unmodified upstream release or a temporary fork awaiting
upstream selection support. See [ADR-0035](../../docs/adr/0035-vendor-gpui-terminal.md).

The upstream component provides a terminal emulator component for
[GPUI](https://gpui.rs) applications using official
[alacritty_terminal](https://docs.rs/alacritty_terminal) for VTE parsing and
emulator state. The general usage and configuration below describe that
component; Kagi-specific integration is distinguished separately.

<img width="1763" alt="Image" src="https://github.com/user-attachments/assets/713ed2b3-e08d-4ff5-8aeb-2d02660ecf7d" />

## Features

- Full ANSI escape sequence support
- 16 ANSI colors, 256-color mode, and 24-bit true color
- Bold, italic, and underline text styles
- Keyboard input with application cursor mode (vim/tmux compatible)
- Clipboard integration via OSC 52
- Dynamic configuration (font size, colors)
- Push-based async I/O
- Accepts any `Read`/`Write` streams (not tied to a specific PTY library)

## Usage

```rust
use gpui_terminal::{TerminalConfig, TerminalView, ColorPalette};

// Create terminal with PTY reader/writer
let terminal = cx.new(|cx| {
    TerminalView::new(pty_writer, pty_reader, config, cx)
        .with_resize_callback(|cols, rows| {
            // Notify PTY of size change
        })
        .with_exit_callback(|_window, cx| {
            cx.quit();
        })
});
```

See `src/main.rs` for a complete example using `portable-pty`.

## Configuration

```rust
let colors = ColorPalette::builder()
    .background(0x16, 0x16, 0x17)
    .foreground(0xC9, 0xC7, 0xCD)
    .cursor(0xC9, 0xC7, 0xCD)
    .black(0x10, 0x10, 0x10)
    // ... other colors
    .build();

let config = TerminalConfig {
    font_family: "Mononoki Nerd Font".into(),
    font_size: px(14.0),
    font_fallbacks: None, // platform defaults, or Some(gpui::FontFallbacks)
    cols: 80,
    rows: 24,
    scrollback: 10000,
    line_height_multiplier: 1.05,
    padding: Edges::all(px(8.0)),
    colors,
};
```

## Kagi-maintained integration

- Mouse text selection, clipboard copy and scrollback navigation are implemented.
- Generic paste handling uses the emulator's bracketed-paste mode and refuses
  unapproved plain multiline/control-byte payloads before writing PTY bytes.
  Kagi owns localized confirmation and session approval, not the emulator.
- `TerminalConfig::font_fallbacks` optionally supplies shared explicit fallback
  families for both cell measurement and normal/bold/italic glyph shaping.
  The generic default remains `None` (platform fallback). Kagi supplies the
  bundled UI fallback stack while preserving the selected monospace/Nerd primary.
  Changing it through the existing `TerminalView::update_config` path takes
  effect on the next paint, which remeasures cells before resizing the grid.
- Frontend and transport changes are maintained here; this ownership decision
  does not replace `alacritty_terminal` or introduce a new emulator.
- Existing `// kagi:` markers identify local modifications. Upstream changes
  remain useful review inputs, not an automatic package-replacement trigger.

## License and attribution

Derived from Leonard Seibold's gpui-terminal 0.1.0:
<https://github.com/zortax/gpui-terminal>.
The imported registry package recorded upstream source commit
`45c63e57181d27c260124a81c7e4b68a6b6e57b0`.

MIT OR Apache-2.0. Keep both [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE), including their upstream attribution, when
distributing this package. Kagi maintenance does not replace those notices
with Kagi-only authorship. Zed's GPL terminal is not the source of this code.
