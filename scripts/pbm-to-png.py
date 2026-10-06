#!/usr/bin/env python3
"""Convert binary P4 host observations to grayscale PNG using only stdlib."""
import binascii
from pathlib import Path
import struct
import sys
import zlib


def chunk(tag, payload):
    return (struct.pack(">I", len(payload)) + tag + payload
            + struct.pack(">I", binascii.crc32(tag + payload) & 0xffffffff))


for path in Path(sys.argv[1]).glob("*.pbm"):
    magic, shape, data = path.read_bytes().split(b"\n", 2)
    width, height = map(int, shape.split())
    stride = (width + 7) // 8
    if magic != b"P4" or len(data) != stride * height:
        raise ValueError(f"unsupported or truncated PBM: {path}")
    rows = b"".join(b"\0" + bytes(
        0 if data[y * stride + x // 8] & (128 >> (x % 8)) else 255
        for x in range(width)) for y in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    path.with_suffix(".png").write_bytes(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
