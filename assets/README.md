# `assets/`

Bundled, non-code resources. Layout:

| Path | What |
| --- | --- |
| `icon-512x512.png` | **Source** app-icon image (square PNG with alpha). Edit this to change the icon. |
| `icon/` | **Generated** icon set — do not edit by hand; regenerate (below). |
| `icons/` | UI SVG glyphs used by the app at runtime. |
| `fonts/` | Bundled UI / monospace fonts. |

## Regenerating the app icon

The icon set in `icon/` is generated from the single source `icon-512x512.png`
by `scripts/make_icon.sh` (ADR-0047). After editing the source, regenerate:

```sh
# from the repo root — either form works:
cargo run -p xtask -- icon      # wraps the script
# or
scripts/make_icon.sh            # defaults to assets/icon-512x512.png
# (pass a different master explicitly, e.g. a 1024² PNG:)
scripts/make_icon.sh path/to/master-1024.png
```

Then commit the source **and** the regenerated `icon/` outputs together.

### What it produces (in `assets/icon/`)
- `AppIcon.icns` — macOS app icon (used by `xtask bundle-macos`).
- `icon_512x512.png`, `icon_256x256.png`, `icon_128x128.png` — Linux hicolor PNGs
  (used by `bundle-linux` / `bundle-appimage`).
- `icon-rounded-1024.png` — the rounded 1024² master (intermediate, kept in git).

### How it works
- Apple-style **continuous rounded corners** are baked in by
  `scripts/round_icon.swift` (CoreGraphics; inset 0.82, radius 0.2237) — so the
  source should be a **full-bleed square** image; the script adds the rounding.
- All sizes are downscaled from the rounded 1024² master, so every output is
  consistent. The pipeline is **idempotent** — re-running reproduces the outputs.

### Requirements
- **macOS only** — uses the stock tools `swift`, `sips`, and `iconutil` (no
  ImageMagick or other third-party deps). On other platforms, regenerate on a
  macOS host and commit the resulting `assets/icon/` files.
