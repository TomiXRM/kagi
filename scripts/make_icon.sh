#!/usr/bin/env bash
# make_icon.sh — Kagi icon pipeline (ADR-0047, W20-RELEASE).
#
# Produces, from a single source PNG (alpha):
#   assets/icon/AppIcon.icns        macOS app icon (iconutil)
#   assets/icon/icon_512x512.png    Linux hicolor 512
#   assets/icon/icon_256x256.png    Linux hicolor 256
#   assets/icon/icon_128x128.png    Linux hicolor 128
#   assets/icon/icon-rounded-1024.png  the rounded master (intermediate, kept)
#
# Apple-style continuous rounded corners are baked in via scripts/round_icon.swift
# (CoreGraphics; no ImageMagick / third-party deps). macOS-stock tools only:
# swift, sips, iconutil.
#
# Idempotent: re-running regenerates all outputs deterministically.
#
# Usage:
#   scripts/make_icon.sh [INPUT_PNG]
# INPUT_PNG defaults to assets/icon-512x512.png. Pass a 1024² master later to
# swap the source with no other change.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

INPUT="${1:-$ROOT_DIR/assets/icon-512x512.png}"
OUT_DIR="$ROOT_DIR/assets/icon"
ROUNDED="$OUT_DIR/icon-rounded-1024.png"

for tool in swift sips iconutil; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' not found (macOS-stock tool required)" >&2; exit 1; }
done

[ -f "$INPUT" ] || { echo "error: input image not found: $INPUT" >&2; exit 1; }

mkdir -p "$OUT_DIR"

echo "make_icon: source = $INPUT"

# 1) Apple-style rounded 1024² master (transparent PNG).
swift "$SCRIPT_DIR/round_icon.swift" "$INPUT" "$ROUNDED" 1024 0.82 0.2237

# 2) macOS .icns via iconset (16..512 @1x/@2x), downscaled from the 1024 master.
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$ROUNDED" --out "$ICONSET/icon_${s}x${s}.png"   >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$ROUNDED" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$OUT_DIR/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"

# 3) Linux hicolor PNGs (512/256/128) from the rounded master.
for s in 512 256 128; do
  sips -z "$s" "$s" "$ROUNDED" --out "$OUT_DIR/icon_${s}x${s}.png" >/dev/null
done

echo "make_icon: wrote:"
echo "  $OUT_DIR/AppIcon.icns"
echo "  $OUT_DIR/icon_512x512.png"
echo "  $OUT_DIR/icon_256x256.png"
echo "  $OUT_DIR/icon_128x128.png"
echo "  $ROUNDED (rounded master)"
