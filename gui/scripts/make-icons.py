"""Draw the app icons (src-tauri/icons): the logo, a canary-yellow rounded
square with a dark lightning bolt, the same shape as the Logo component and
the panel icon. Run from gui/: python3 scripts/make-icons.py"""
from PIL import Image, ImageDraw

YELLOW = (0xFF, 0xEF, 0x00, 255)
BOLT = (0x1B, 0x1D, 0x1F, 255)
# In the logo's own 28x28 units (see components.tsx Logo).
BOLT_PTS = [(15.5, 6.0), (9.0, 15.5), (14.0, 15.5), (12.5, 22.0), (19.0, 12.5), (14.0, 12.5)]


def icon(size: int) -> Image.Image:
    ss = 8  # supersampling, for smooth edges
    big = size * ss
    k = big / 28.0
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([1 * k, 1 * k, 27 * k, 27 * k], radius=8 * k, fill=YELLOW)
    d.polygon([(x * k, y * k) for x, y in BOLT_PTS], fill=BOLT)
    return im.resize((size, size), Image.LANCZOS)


for name, size in [("icon.png", 512), ("128x128@2x.png", 256), ("128x128.png", 128), ("32x32.png", 32)]:
    icon(size).save(f"src-tauri/icons/{name}")
    print(name, size)
