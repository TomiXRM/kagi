#!/usr/bin/env python3
"""Regenerate the bundled static Noto Sans JP faces from the variable source.

Kagi ships *static* Regular(400)/Bold(700) instances of Noto Sans JP rather than
the upstream variable font. On Linux GPUI's cosmic-text renders a *variable*
fallback at the font's default axis value, and Noto Sans JP's variable default is
Thin (``wght=100``) — so Japanese text rendered thin regardless of the requested
weight (ADR-0130). Static faces carry the weight in ``OS/2.usWeightClass`` and let
fontdb resolve the requested weight directly, exactly like the bundled Inter and
JetBrains Mono pairs.

Usage (from the repo root)::

    python3 scripts/instance_noto.py path/to/NotoSansJP[wght].ttf

Writes ``assets/fonts/NotoSansJP-Regular.ttf`` and ``assets/fonts/NotoSansJP-Bold.ttf``.
Requires ``fonttools`` (``pip install fonttools``).
"""

import sys
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

FAMILY = "Noto Sans JP"
OUT_DIR = Path("assets/fonts")

# Pin the head-table timestamps so instancing is byte-reproducible (otherwise
# fontTools stamps `modified` with the current time and the SHA-256 drifts every
# run). Value is seconds since the 1904-01-01 Mac epoch — an arbitrary fixed date.
FIXED_TIMESTAMP = 0

# fsSelection bits (OS/2). USE_TYPO_METRICS matches the bundled Inter faces.
FS_ITALIC = 0x01
FS_BOLD = 0x20
FS_REGULAR = 0x40
FS_USE_TYPO_METRICS = 0x80


def _set_name(font: TTFont, name_id: int, value: str) -> None:
    font["name"].setName(value, name_id, 3, 1, 0x409)  # Windows / Unicode / en-US
    font["name"].setName(value, name_id, 1, 0, 0)  # Mac / Roman / English


def _make(src: Path, weight: int, subfamily: str, bold: bool) -> Path:
    font = TTFont(str(src))
    instantiateVariableFont(font, {"wght": float(weight)}, inplace=True)

    os2, head = font["OS/2"], font["head"]
    os2.usWeightClass = weight
    fs = os2.fsSelection & ~(FS_ITALIC | FS_BOLD | FS_REGULAR)
    fs |= FS_USE_TYPO_METRICS
    fs |= FS_BOLD if bold else FS_REGULAR
    os2.fsSelection = fs
    head.macStyle = (head.macStyle & ~0b11) | (0b01 if bold else 0)
    head.created = FIXED_TIMESTAMP
    head.modified = FIXED_TIMESTAMP

    _set_name(font, 1, FAMILY)  # family
    _set_name(font, 2, subfamily)  # subfamily
    _set_name(font, 4, f"{FAMILY} {subfamily}" if bold else FAMILY)  # full name
    _set_name(font, 6, f"NotoSansJP-{subfamily}")  # PostScript name
    for typographic_id in (16, 17):  # drop typographic family/subfamily -> plain RIBBI
        font["name"].removeNames(nameID=typographic_id)

    out = OUT_DIR / f"NotoSansJP-{subfamily}.ttf"
    font.save(str(out))
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    src = Path(sys.argv[1])
    if not src.is_file():
        print(f"source variable font not found: {src}", file=sys.stderr)
        return 1
    for weight, subfamily, bold in ((400, "Regular", False), (700, "Bold", True)):
        out = _make(src, weight, subfamily, bold)
        print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
