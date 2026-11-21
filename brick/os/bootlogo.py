"""Render the official SLOT wordmark as a TrimUI Slot OS bootloader logo.

High-resolution wordmark: slot-for-android's slot-bootlogo-kpa-960x640.png.
The uncompressed BMP V5 output format matches TrimUI u-boot.

Usage: python3 brick/os/bootlogo.py <output.bmp>
"""
import struct
import sys
from pathlib import Path

from PIL import Image

W, H = 1024, 768
SOURCE = Path(__file__).resolve().parents[2] / "assets/android-original/slot-bootlogo-kpa-960x640.png"
WORDMARK_SIZE = (314, 120)

with Image.open(SOURCE) as image:
    luminance = image.convert("L")
# Lift the wordmark off the source's dark backdrop and preserve its edge coverage.
alpha = luminance.point(lambda value: round(max(0, min(1, (value - 22) / (242 - 22))) * 255))
alpha = alpha.crop(alpha.getbbox()).resize(WORDMARK_SIZE, Image.Resampling.LANCZOS)
screen = Image.new("RGB", (W, H), (0, 0, 0))
screen.paste((244, 245, 249), ((W - alpha.width) // 2, (H - alpha.height) // 2), alpha)

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
