"""Render the official SLOT wordmark as a TrimUI Slot OS bootloader logo.

Original art: slot-cfw/slot, card/System/bootlogo.bmp (GPL-3.0-or-later).
The uncompressed BMP V5 output format matches TrimUI u-boot.

Usage: python3 brick/os/bootlogo.py <output.bmp>
"""
import struct
import sys
from pathlib import Path

from PIL import Image

W, H = 1024, 768
SOURCE = Path(__file__).with_name("slot-wordmark-upstream.bmp")

wordmark = Image.open(SOURCE).convert("RGB")
# Match the upstream wordmark's visual size relative to a 640x480 screen.
wordmark = wordmark.resize(
    (round(wordmark.width * W / 640), round(wordmark.height * H / 480)),
    Image.Resampling.NEAREST,
)
screen = Image.new("RGB", (W, H), (0, 0, 0))
screen.paste(wordmark, ((W - wordmark.width) // 2, (H - wordmark.height) // 2))

pixels = b"".join(
    b"".join(bytes((b, g, r, 255)) for r, g, b in (
        screen.getpixel((x, y)) for x in range(W)
    ))
    for y in range(H - 1, -1, -1)
)
v5 = struct.pack("<IiiHHIIiiII", 124, W, H, 1, 32, 3, len(pixels), 2835, 2835, 0, 0)
v5 += struct.pack("<IIII", 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
v5 += b"BGRs" + bytes(36) + struct.pack("<III", 0, 0, 0) + struct.pack("<IIII", 4, 0, 0, 0)
head = b"BM" + struct.pack("<IHHI", 14 + 124 + len(pixels), 0, 0, 14 + 124)
with open(sys.argv[1], "wb") as output:
    output.write(head + v5 + pixels)
