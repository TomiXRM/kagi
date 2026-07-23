# `assets/`

Bundled, non-code resources. Layout:

| Path | What |
| --- | --- |
| `icon-512x512.png` | **Source** app-icon image (square PNG with alpha). Edit this to change the icon. |
| `icon/` | **Generated** icon set — do not edit by hand; regenerate (below). |
| `icons/` | UI SVG glyphs used by the app at runtime. |
| `fonts/` | Bundled UI / monospace fonts. |

## Bundled fonts

| Files | Purpose | License / provenance |
| --- | --- | --- |
| `Inter-*.ttf` | Primary UI Latin font | SIL OFL 1.1; `Inter-OFL.txt` |
| `JetBrainsMono-*.ttf` | Terminal and code | SIL OFL 1.1; `JetBrainsMono-OFL.txt` |
| `NotoSansJP-Regular.ttf`, `NotoSansJP-Bold.ttf` | Deterministic Japanese/CJK fallback | SIL OFL 1.1; static `wght=400`/`700` instances of Google Fonts commit `2f6daa88e1e71320a6fe71cc91ecbfc018928737` `NotoSansJP[wght].ttf`; `NotoSansJP-OFL.txt` |

The Japanese fallback ships as **static** Regular(400)/Bold(700) instances, not
the upstream variable font: on Linux GPUI's cosmic-text renders a *variable*
fallback at its default axis, and Noto Sans JP's variable default is Thin
(`wght=100`), so Japanese text rendered thin (ADR-0130). Static faces carry the
weight in `usWeightClass`, so the requested weight resolves directly — the same
way the bundled Inter/JetBrains Mono pairs are shipped.

Provenance is the upstream Google Fonts variable font at commit
`2f6daa88e1e71320a6fe71cc91ecbfc018928737` (`ofl/notosansjp/NotoSansJP[wght].ttf`,
SHA-256 `c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f`).
Regenerate the two faces from it with `fontTools.varLib.instancer`:

```sh
pip install fonttools
python3 scripts/instance_noto.py path/to/NotoSansJP[wght].ttf
```

As-shipped SHA-256 of the committed faces (integrity fingerprint; fontTools may
not reproduce them byte-for-byte across versions):

- `NotoSansJP-Regular.ttf`: `991c8e65b73a0c9f46b9f6b53354c69952cef6bd675cf181c1dc9a0e41dc1d9b`
- `NotoSansJP-Bold.ttf`: `c441b828cfde42526f3b4961d1fbfd1cde1ef528e872f486370f2e029fd3a6d8`

They are embedded by `src/ui/fonts.rs`, so the native binary, `.deb`, tarball,
and AppImage all use the same glyph data without relying on host-installed fonts.

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
