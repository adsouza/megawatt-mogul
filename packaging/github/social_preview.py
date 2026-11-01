"""Compose the 1280x640 GitHub social preview from the package icon.

Usage: python3 -I packaging/github/social_preview.py <bold-font> <regular-font> <output.png>

The fonts used for the committed image are Atkinson Hyperlegible Next ExtraBold and Regular
(SIL Open Font License). Run from the repository root.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

WIDTH, HEIGHT = 1280, 640
ICON = Path("packaging/linux/icon.png")
# Navy sampled from the icon's tile, so the tile edge sits quietly on the background.
BACKGROUND_TOP = (10, 20, 37)
BACKGROUND_BOTTOM = (20, 34, 58)
GLOW = (255, 196, 64)
TITLE = (246, 248, 252)
ACCENT = (255, 205, 74)
TAGLINE = (176, 194, 222)
# Matches menu-subtitle in assets/locales/en-US/ui.ftl.
TAGLINE_LINES = ("Build your grid.", "Keep the lights on.")


def vertical_gradient() -> Image.Image:
    image = Image.new("RGB", (WIDTH, HEIGHT))
    draw = ImageDraw.Draw(image)
    for y in range(HEIGHT):
        t = y / (HEIGHT - 1)
        colour = tuple(round(a + (b - a) * t) for a, b in zip(BACKGROUND_TOP, BACKGROUND_BOTTOM))
        draw.line([(0, y), (WIDTH, y)], fill=colour)
    return image.convert("RGBA")


def soft_ellipse(box, colour, alpha, blur) -> Image.Image:
    layer = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse(box, fill=(*colour, alpha))
    return layer.filter(ImageFilter.GaussianBlur(blur))


def main(bold_font: str, regular_font: str, output: str) -> None:
    canvas = vertical_gradient()

    icon = Image.open(ICON).convert("RGBA")
    icon = icon.crop(icon.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox())
    size = 500
    icon = icon.resize((round(icon.width * size / icon.height), size), Image.LANCZOS)
    left, top = 86, (HEIGHT - size) // 2
    centre = (left + icon.width // 2, top + size // 2)

    canvas.alpha_composite(
        soft_ellipse(
            (centre[0] - 300, centre[1] - 260, centre[0] + 300, centre[1] + 260), GLOW, 70, 90
        )
    )
    shadow = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 150), (left, top + 18), icon.getchannel("A"))
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(22)))
    canvas.alpha_composite(icon, (left, top))

    draw = ImageDraw.Draw(canvas)
    title_font = ImageFont.truetype(bold_font, 116)
    tagline_font = ImageFont.truetype(regular_font, 44)
    x = 652
    y = 138
    for word in ("Megawatt", "Mogul"):
        draw.text((x - 4, y), word, font=title_font, fill=TITLE)
        y += 120
    y += 34
    draw.rounded_rectangle((x, y, x + 96, y + 8), radius=4, fill=ACCENT)
    y += 38
    for line in TAGLINE_LINES:
        draw.text((x, y), line, font=tagline_font, fill=TAGLINE)
        y += 56

    canvas.convert("RGB").save(output, optimize=True)


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
