#!/usr/bin/env python3
"""Concatenate two sips-produced, uncompressed BMPs into a PNG (stdlib only)."""

import struct
import sys
import zlib
from pathlib import Path


def read_bmp(path):
    data = Path(path).read_bytes()
    if len(data) < 54 or data[:2] != b"BM":
        raise ValueError(f"{path}: not a BMP file")
    pixel_offset = struct.unpack_from("<I", data, 10)[0]
    header_size = struct.unpack_from("<I", data, 14)[0]
    if header_size < 40 or len(data) < 14 + header_size or pixel_offset < 14 + header_size:
        raise ValueError(f"{path}: unsupported BMP header")
    width, signed_height, planes, depth, compression = struct.unpack_from(
        "<iiHHI", data, 18
    )
    if width <= 0 or signed_height == 0 or planes != 1 or depth not in (24, 32):
        raise ValueError(f"{path}: unsupported BMP dimensions or depth")
    if compression not in (0, 3) or (compression == 3 and depth != 32):
        raise ValueError(f"{path}: BMP must be uncompressed RGB or 32-bit bitfields")
    height = abs(signed_height)
    stride = ((width * depth + 31) // 32) * 4
    if len(data) < pixel_offset + height * stride:
        raise ValueError(f"{path}: truncated BMP pixels")

    masks = None
    if compression == 3:
        mask_offset = 14 + (40 if header_size >= 52 else header_size)
        if pixel_offset < mask_offset + 12:
            raise ValueError(f"{path}: missing RGB bitfield masks")
        rgb = struct.unpack_from("<III", data, mask_offset)
        alpha = (
            struct.unpack_from("<I", data, mask_offset + 12)[0]
            if (header_size >= 56 or pixel_offset >= mask_offset + 16)
            else 0
        )
        masks = (*rgb, alpha)
        if not all(rgb) or rgb[0] & rgb[1] or rgb[0] & rgb[2] or rgb[1] & rgb[2]:
            raise ValueError(f"{path}: invalid RGB bitfield masks")

    rows = []
    for y in range(height):
        source_y = y if signed_height < 0 else height - 1 - y
        offset = pixel_offset + source_y * stride
        row = bytearray(width * 4)
        for x in range(width):
            start = offset + x * (depth // 8)
            if masks is None:
                blue, green, red = data[start : start + 3]
                rgba = (red, green, blue, 255)
            else:
                pixel = struct.unpack_from("<I", data, start)[0]
                rgba = tuple(component(pixel, mask) for mask in masks[:3]) + (
                    component(pixel, masks[3]) if masks[3] else 255,
                )
            row[4 * x : 4 * x + 4] = bytes(rgba)
        rows.append(row)
    return width, height, rows


def component(pixel, mask):
    shift = (mask & -mask).bit_length() - 1
    maximum = mask >> shift
    return ((pixel & mask) >> shift) * 255 // maximum


def chunk(name, contents):
    return (
        struct.pack(">I", len(contents))
        + name
        + contents
        + struct.pack(">I", zlib.crc32(name + contents) & 0xFFFFFFFF)
    )


def compose(left, right, destination):
    lw, lh, left_rows = read_bmp(left)
    rw, rh, right_rows = read_bmp(right)
    width, height = lw + rw, max(lh, rh)
    blank_left = bytes(lw * 4)
    blank_right = bytes(rw * 4)
    raw = b"".join(
        b"\0"
        + (left_rows[y] if y < lh else blank_left)
        + (right_rows[y] if y < rh else blank_right)
        for y in range(height)
    )
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )
    Path(destination).write_bytes(png)


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit("usage: inventory-diff.py <before.bmp> <after.bmp> <output.png>")
    try:
        compose(*sys.argv[1:])
    except (OSError, ValueError) as error:
        sys.exit(f"inventory-diff: {error}")
