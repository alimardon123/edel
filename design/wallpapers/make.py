#!/usr/bin/env python3
"""Makes the wallpapers the `wallpapers` feature carries (M5.10c4b).

Each wallpaper is a pair, NAME-light.jpg and NAME-dark.jpg, and the
compositor shows the one the colour scheme names. Two pairs are our own,
drawn here as SVG from design/tokens.toml's colours, so they follow the
theme: Hills (layered hills under a dawn sky, and under a night sky with
stars) and Aurora (soft ribbons of light). The third, Earth, is NASA's
Europe by day and by night, cut from two public domain maps (CREDITS).

    python3 design/wallpapers/make.py OUT_DIR
    python3 design/wallpapers/make.py OUT_DIR --earth DAY.jpg NIGHT.jpg

Needs Python 3.11 or later, cargo (the SVG is drawn by the example
`render-svg`, with the resvg our icons are drawn with) and ImageMagick's
`convert` for the JPEG files. The output is the same on every run.
"""

import math
import random
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
W, H = 3840, 2400
QUALITY = 90


def tokens():
    with open(ROOT / "design" / "tokens.toml", "rb") as f:
        t = tomllib.load(f)
    return t["colour"], t["colour"]["light"]


def rgb(hex_colour):
    h = hex_colour.lstrip("#")[:6]
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def mix(a, b, t):
    """`a` moved `t` of the way to `b`, both as #rrggbb."""
    ca, cb = rgb(a), rgb(b)
    return "#" + "".join(f"{round(x + (y - x) * t):02x}" for x, y in zip(ca, cb))


def ramp(near, far, n):
    """`n` colours from `far` (the first, the farthest layer) to `near`."""
    return [mix(far, near, i / (n - 1)) for i in range(n)]


def wave(base, amp, seed):
    """The top edge of a hill: a sum of three sines, the same for a seed."""
    rnd = random.Random(seed)
    parts = [
        (rnd.uniform(0.6, 1.6) * k, rnd.uniform(0, math.tau), amp / (k * 1.3))
        for k in (1, 2.3, 4.1)
    ]
    points = []
    for x in range(0, W + 41, 40):
        y = base + sum(a * math.sin(math.tau * f * x / W + p) for f, p, a in parts)
        points.append(f"{x},{y:.1f}")
    return f"M0,{H} L" + " L".join(points) + f" L{W},{H} Z"


def stars(seed, count, top):
    rnd = random.Random(seed)
    return [
        f'<circle cx="{rnd.uniform(0, W):.0f}" cy="{rnd.uniform(0, H * top):.0f}" '
        f'r="{rnd.choice([1.2, 1.6, 2.2, 2.8])}" fill="#ffffff" '
        f'opacity="{rnd.uniform(0.2, 0.75):.2f}"/>'
        for _ in range(count)
    ]


def hills(sky, glow, near, far, shade, shade_alpha, starry):
    """Five hills from far to near under a sky with a soft glow above
    the horizon; each hill a vertical gradient with a soft shadow above."""
    layers = ramp(near, far, 5)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="0.75" stop-color="{sky[1]}"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="glow" cx="0.68" cy="0.42" r="0.45"><stop offset="0" '
        f'stop-color="{glow}" stop-opacity="0.85"/><stop offset="1" stop-color="{glow}" '
        'stop-opacity="0"/></radialGradient>'
    )
    for i, colour in enumerate(layers):
        out.append(
            f'<linearGradient id="l{i}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
            f'stop-color="{colour}"/><stop offset="1" stop-color="{mix(colour, near, 0.15)}"/>'
            "</linearGradient>"
        )
    out.append(
        '<filter id="shade" x="-5%" y="-10%" width="110%" height="120%">'
        '<feGaussianBlur stdDeviation="40"/></filter></defs>'
    )
    out.append(f'<rect width="{W}" height="{H}" fill="url(#sky)"/>')
    out.append(f'<rect width="{W}" height="{H}" fill="url(#glow)"/>')
    if starry:
        out += stars(7, 260, 0.55)
    for i, (base, amp) in enumerate([(0.60, 0.05), (0.67, 0.055), (0.75, 0.05), (0.83, 0.04), (0.91, 0.03)]):
        d = wave(base * H, amp * H, i + 1)
        out.append(
            f'<path d="{d}" fill="{shade}" opacity="{shade_alpha}" '
            'transform="translate(0,-18)" filter="url(#shade)"/>'
        )
        out.append(f'<path d="{d}" fill="url(#l{i})"/>')
    out.append("</svg>")
    return "\n".join(out)


def ribbon(y0, amp, freq, phase, thick, tilt):
    top, bottom = [], []
    for x in range(0, W + 41, 40):
        u = x / W
        y = (
            y0
            + tilt * (u - 0.5) * H
            + amp * math.sin(math.tau * freq * u + phase)
            + 0.35 * amp * math.sin(math.tau * 2.7 * freq * u + phase * 1.7)
        )
        t = thick * (0.55 + 0.45 * math.sin(math.pi * u + phase))
        top.append((x, y - t / 2))
        bottom.append((x, y + t / 2))
    return "M" + " L".join(f"{x},{y:.1f}" for x, y in top + bottom[::-1]) + " Z"


# Where each ribbon lies: its middle, its swing, how often it swings, its
# phase, its thickness, its tilt and its strength.
RIBBONS = [
    (0.38, 0.08, 0.8, 0.4, 0.22, -0.25, 0.9),
    (0.55, 0.06, 1.1, 2.1, 0.16, -0.18, 0.8),
    (0.70, 0.05, 0.9, 4.0, 0.12, -0.10, 0.7),
]


def aurora(sky, colours, starry):
    """Three soft ribbons of light, each fading in and out along its
    length, with a brighter thread down its middle."""
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0.3" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="1" stop-color="{sky[1]}"/></linearGradient>'
    )
    for i, (a, b) in enumerate(colours):
        out.append(
            f'<linearGradient id="r{i}" x1="0" y1="0" x2="1" y2="0">'
            f'<stop offset="0" stop-color="{a}" stop-opacity="0"/><stop offset="0.35" stop-color="{a}"/>'
            f'<stop offset="0.7" stop-color="{b}"/><stop offset="1" stop-color="{b}" stop-opacity="0"/>'
            "</linearGradient>"
        )
    for name, blur in (("wide", 120), ("thin", 30)):
        out.append(
            f'<filter id="{name}" x="-20%" y="-150%" width="140%" height="400%">'
            f'<feGaussianBlur stdDeviation="{blur}"/></filter>'
        )
    out.append("</defs>")
    out.append(f'<rect width="{W}" height="{H}" fill="url(#sky)"/>')
    if starry:
        out += stars(11, 320, 1.0)
    for i, (y0, amp, freq, phase, thick, tilt, strength) in enumerate(RIBBONS):
        wide = ribbon(y0 * H, amp * H, freq, phase, thick * H, tilt)
        thin = ribbon(y0 * H, amp * H, freq, phase, thick * H * 0.18, tilt)
        out.append(f'<path d="{wide}" fill="url(#r{i})" opacity="{strength}" filter="url(#wide)"/>')
        out.append(f'<path d="{thin}" fill="url(#r{i})" opacity="{strength * 0.7:.2f}" filter="url(#thin)"/>')
    out.append("</svg>")
    return "\n".join(out)


def ours():
    """Our own pairs, from the tokens: the dark ones from the lock screen's
    navy and glows (`lock_backdrop`, `lock_backdrop_deep`,
    `lock_glow_second`) and the accent, the light ones from the light
    scheme's accent over a dawn."""
    dark, light = tokens()
    night = "#0b1024"
    dawn = "#fbe3d6"
    accent, accent_light = dark["accent"], light["accent"]
    return {
        "hills-dark": hills(
            sky=(mix(dark["lock_backdrop_deep"], night, 0.5), mix(dark["lock_glow_second"], "#6a5aa8", 0.2)),
            glow=mix(dark["lock_glow_second"], "#ffffff", 0.3),
            near=mix(dark["lock_backdrop_deep"], night, 0.7),
            far=mix(dark["lock_backdrop"], accent, 0.1),
            shade="#000000",
            shade_alpha=0.18,
            starry=True,
        ),
        "hills-light": hills(
            sky=(mix(accent_light, "#ffffff", 0.82), dawn),
            glow="#ffd2b8",
            near=mix(accent_light, "#2a3570", 0.35),
            far=mix(accent_light, "#ffffff", 0.78),
            shade=mix(accent_light, "#1a2050", 0.5),
            shade_alpha=0.12,
            starry=False,
        ),
        "aurora-dark": aurora(
            sky=(night, dark["lock_backdrop_deep"]),
            colours=[("#38c8b4", accent), ("#7a5bd0", "#38c8b4"), (accent, "#c06bd0")],
            starry=True,
        ),
        "aurora-light": aurora(
            sky=(mix(accent_light, "#ffffff", 0.92), mix(accent_light, "#ffffff", 0.84)),
            colours=[("#5fd0bd", mix(accent_light, "#ffffff", 0.35)), ("#a98cff", "#62d3bf"), (mix(accent_light, "#ffffff", 0.45), "#e48fd0")],
            starry=False,
        ),
    }


def run(*args):
    subprocess.run(args, check=True)


def draw(out, name, svg):
    svg_path = out / f"{name}.svg"
    png_path = out / f"{name}.png"
    svg_path.write_text(svg)
    run(
        "cargo", "run", "-q", "--manifest-path", str(ROOT / "Cargo.toml"),
        "-p", "edel", "--features", "icons", "--example", "render-svg", "--",
        str(svg_path), str(png_path),
    )
    run("convert", str(png_path), "-quality", str(QUALITY), str(out / f"{name}.jpg"))
    svg_path.unlink()
    png_path.unlink()


# The Earth pair: longitude 12 W to 60 E, latitude 30 N to 63 N (Europe,
# the Mediterranean and the Middle East), from two equirectangular maps of
# the whole Earth, drawn at 2560x1600.
EARTH = (-12.0, 60.0, 30.0, 63.0)
EARTH_SIZE = "2560x1600"


def earth(out, day, night):
    lon0, lon1, lat0, lat1 = EARTH
    for name, source in (("earth-light", day), ("earth-dark", night)):
        width = int(subprocess.run(
            ("identify", "-format", "%w", str(source)), check=True, capture_output=True, text=True
        ).stdout)
        per_degree = width / 360
        x, y = int((lon0 + 180) * per_degree), int((90 - lat1) * per_degree)
        w, h = int((lon1 - lon0) * per_degree), int((lat1 - lat0) * per_degree)
        run(
            "convert", "-limit", "memory", "2GiB", str(source),
            "-crop", f"{w}x{h}+{x}+{y}", "+repage", "-resize", f"{EARTH_SIZE}!",
            "-quality", str(QUALITY), str(out / f"{name}.jpg"),
        )


def main(argv):
    if not argv or argv[0].startswith("-"):
        sys.exit(__doc__)
    out = Path(argv[0])
    out.mkdir(parents=True, exist_ok=True)
    for name, svg in ours().items():
        draw(out, name, svg)
    if len(argv) == 4 and argv[1] == "--earth":
        earth(out, Path(argv[2]), Path(argv[3]))


if __name__ == "__main__":
    main(sys.argv[1:])
