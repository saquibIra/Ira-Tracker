"""Generates every Time Pilot brand asset from one drawing: web favicons and logos,
email logo, desktop app icon source and tray icons.

    python3 scripts/brand/make_brand.py        # needs Pillow (pip install pillow)

InterVariable.ttf is Inter by Rasmus Andersson (SIL Open Font License 1.1), the font
the web app also uses.

The mark is a rounded square with a dial whose minute hand ends in a navigation
arrow: time, piloted. Colours live in BRAND below and in the web/desktop themes.
"""

from __future__ import annotations

import json
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONT = Path(__file__).with_name("InterVariable.ttf")
WEB = ROOT / "server" / "web" / "overlay" / "public"
DESKTOP = ROOT / "widget" / "src-tauri" / "icons"

BRAND = {
    "teal": (13, 148, 136),  # #0D9488, the mark
    "teal_dark": (15, 118, 110),  # #0F766E
    "ink": (15, 23, 42),  # #0F172A, wordmark on light
    "white": (255, 255, 255),
    "grey": (100, 116, 139),  # tray icon when stopped
}
HEX = {k: "#{:02X}{:02X}{:02X}".format(*v) for k, v in BRAND.items()}

# Geometry in a 48 x 48 box (same numbers as the SVG below).
HAND_ANGLE = math.radians(-30)  # minute hand: towards 2 o'clock


def _pt(r: float, angle: float = HAND_ANGLE) -> tuple[float, float]:
    return 24 + r * math.cos(angle), 24 + r * math.sin(angle)


def mark_svg(bg: str = HEX["teal"], fg: str = "#FFFFFF") -> str:
    tip = _pt(14.0)
    base = _pt(9.6)
    perp = HAND_ANGLE + math.pi / 2
    b1 = (base[0] + 3.3 * math.cos(perp), base[1] + 3.3 * math.sin(perp))
    b2 = (base[0] - 3.3 * math.cos(perp), base[1] - 3.3 * math.sin(perp))
    hand_end = _pt(10.4)
    f = lambda p: f"{p[0]:.2f} {p[1]:.2f}"  # noqa: E731
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48" fill="none">'
        f'<rect width="48" height="48" rx="12" fill="{bg}"/>'
        f'<circle cx="24" cy="24" r="14.5" stroke="{fg}" stroke-opacity="0.35" stroke-width="3"/>'
        f'<path d="M24 24V14" stroke="{fg}" stroke-width="3.2" stroke-linecap="round"/>'
        f'<path d="M24 24L{f(hand_end)}" stroke="{fg}" stroke-width="3.2" stroke-linecap="round"/>'
        f'<path d="M{f(tip)}L{f(b1)}L{f(b2)}Z" fill="{fg}"/>'
        f'<circle cx="24" cy="24" r="3" fill="{fg}"/>'
        "</svg>"
    )


def draw_mark(size: int, bg=BRAND["teal"], fg=BRAND["white"], radius: float = 12, ring_alpha: int = 90) -> Image.Image:
    """The mark as RGBA at `size` px, drawn at 4x and downsampled for smooth edges."""
    ss = 4
    s = size * ss / 48.0
    img = Image.new("RGBA", (size * ss, size * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if bg is not None:
        d.rounded_rectangle([0, 0, size * ss - 1, size * ss - 1], radius=radius * s, fill=bg + (255,))
    ring = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(ring).ellipse(
        [(24 - 14.5) * s, (24 - 14.5) * s, (24 + 14.5) * s, (24 + 14.5) * s],
        outline=fg + (ring_alpha,),
        width=round(3 * s),
    )
    img = Image.alpha_composite(img, ring)
    d = ImageDraw.Draw(img)
    w = round(3.2 * s)

    def line(a, b):
        d.line([a[0] * s, a[1] * s, b[0] * s, b[1] * s], fill=fg + (255,), width=w)
        for p in (a, b):
            d.ellipse([p[0] * s - w / 2, p[1] * s - w / 2, p[0] * s + w / 2, p[1] * s + w / 2], fill=fg + (255,))

    line((24, 24), (24, 14))
    line((24, 24), _pt(10.4))
    tip, base = _pt(14.0), _pt(9.6)
    perp = HAND_ANGLE + math.pi / 2
    b1 = (base[0] + 3.3 * math.cos(perp), base[1] + 3.3 * math.sin(perp))
    b2 = (base[0] - 3.3 * math.cos(perp), base[1] - 3.3 * math.sin(perp))
    d.polygon([(tip[0] * s, tip[1] * s), (b1[0] * s, b1[1] * s), (b2[0] * s, b2[1] * s)], fill=fg + (255,))
    d.ellipse([(24 - 3) * s, (24 - 3) * s, (24 + 3) * s, (24 + 3) * s], fill=fg + (255,))
    return img.resize((size, size), Image.LANCZOS)


def font(px: int, weight: int = 650) -> ImageFont.FreeTypeFont:
    """Inter at `weight`, with the display optical size (Inter has both axes)."""
    f = ImageFont.truetype(str(FONT), px)
    values = []
    for axis in f.get_variation_axes():
        name = axis["name"].decode() if isinstance(axis["name"], bytes) else str(axis["name"])
        values.append(weight if name.lower() == "weight" else axis["maximum"])
    f.set_variation_by_axes(values)
    return f


def wordmark(height: int, color=BRAND["ink"]) -> Image.Image:
    """Mark + "Time Pilot" on a transparent background (email header, docs)."""
    mark = draw_mark(height)
    f = font(round(height * 0.56))
    text = "Time Pilot"
    l, t, r, b = f.getbbox(text)
    gap = round(height * 0.3)
    img = Image.new("RGBA", (height + gap + (r - l) + 4, height), (0, 0, 0, 0))
    img.paste(mark, (0, 0), mark)
    ImageDraw.Draw(img).text((height + gap - l, (height - (b - t)) / 2 - t), text, font=f, fill=color + (255,))
    return img


def app_icon(size: int = 1024) -> Image.Image:
    """macOS-style app icon: the mark on a canvas with the standard ~10% margin."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    inner = round(size * 0.805)
    m = draw_mark(inner, radius=10.5)
    off = (size - inner) // 2
    canvas.paste(m, (off, off), m)
    return canvas


def tray(running: bool, size: int = 64) -> Image.Image:
    """Tray icon: the dial alone, teal while a timer runs, grey when stopped."""
    color = BRAND["teal"] if running else BRAND["grey"]
    return draw_mark(size, bg=None, fg=color, ring_alpha=255 if running else 170)


def main() -> None:
    (WEB / "images").mkdir(parents=True, exist_ok=True)
    (WEB / "favicons").mkdir(parents=True, exist_ok=True)
    (WEB / "images" / "timepilot-mark.svg").write_text(mark_svg())
    wordmark(96).save(WEB / "images" / "timepilot-logo.png")
    wordmark(96, color=BRAND["white"]).save(WEB / "images" / "timepilot-logo-light.png")

    fav = WEB / "favicons"
    for name, px in {
        "favicon-16x16.png": 16,
        "favicon-32x32.png": 32,
        "apple-touch-icon.png": 180,
        "android-chrome-192x192.png": 192,
        "android-chrome-512x512.png": 512,
        "mstile-70x70.png": 70,
        "mstile-144x144.png": 144,
        "mstile-150x150.png": 150,
        "mstile-310x310.png": 310,
    }.items():
        draw_mark(px, radius=0 if name.startswith("apple") else 12).save(fav / name)
    wide = Image.new("RGBA", (310, 150), BRAND["teal"] + (255,))
    m = draw_mark(110)
    wide.paste(m, (100, 20), m)
    wide.save(fav / "mstile-310x150.png")
    draw_mark(64).save(fav / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    (WEB / "favicon.ico").write_bytes((fav / "favicon.ico").read_bytes())
    (fav / "safari-pinned-tab.svg").write_text(mark_svg(bg="#000000", fg="#FFFFFF"))
    (fav / "site.webmanifest").write_text(
        json.dumps(
            {
                "name": "Time Pilot",
                "short_name": "Time Pilot",
                "icons": [
                    {"src": "/favicons/android-chrome-192x192.png", "sizes": "192x192", "type": "image/png"},
                    {"src": "/favicons/android-chrome-512x512.png", "sizes": "512x512", "type": "image/png"},
                ],
                "theme_color": HEX["teal"],
                "background_color": "#FFFFFF",
                "display": "standalone",
            },
            indent=4,
        )
        + "\n"
    )
    (fav / "browserconfig.xml").write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n<browserconfig>\n    <msapplication>\n        <tile>\n'
        '            <square150x150logo src="/favicons/mstile-150x150.png"/>\n'
        f"            <TileColor>{HEX['teal']}</TileColor>\n        </tile>\n    </msapplication>\n</browserconfig>\n"
    )

    DESKTOP.mkdir(parents=True, exist_ok=True)
    app_icon().save(DESKTOP / "app-icon-source.png")
    tray(True).save(DESKTOP / "tray-running.png")
    tray(False).save(DESKTOP / "tray-stopped.png")
    print("brand assets written")


if __name__ == "__main__":
    main()
