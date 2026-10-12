#!/usr/bin/env python3
"""Makes the wallpapers the `wallpapers` feature carries (M5.10c4b).

Each wallpaper is a pair, NAME-light.jpg and NAME-dark.jpg, and the
compositor shows the one the colour scheme names. Two pairs are our own,
drawn here as SVG from design/tokens.toml's colours, so they follow the
theme: Hills (layered hills under a dawn sky, and under a night sky with
stars) and Aurora (soft ribbons of light), with their collections (Ridges
and Mist, Aurora's arc, Ocean's three: the sea from its horizon,
still and from under the surface), each a light and a dark picture. The
third, Earth, is NASA's Europe by day and by night, cut from two public
domain maps (CREDITS).

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
GRAIN = 1.0


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


def soft_tent(x, centre, width):
    """A broad, smooth peak: 1 at its centre, 0 at `width` away, with gentle
    slopes (a tent squared, so it has no corner)."""
    d = min(1.0, abs(x - centre) / width)
    return (1 - d) ** 2


def ridge_points(base, amp, seed):
    """A mountain range's top edge, as points: two or three low sines and two
    broad peaks with gentle slopes, no high-frequency part, so it reads as a
    silhouette and not as noise."""
    rnd = random.Random(seed)
    sines = [
        (rnd.uniform(0.6, 1.0), rnd.uniform(0, math.tau), 0.45),
        (rnd.uniform(1.3, 1.9), rnd.uniform(0, math.tau), 0.22),
        (rnd.uniform(2.2, 2.8), rnd.uniform(0, math.tau), 0.1),
    ]
    peaks = [
        (rnd.uniform(0.15, 0.85) * W, rnd.uniform(0.22, 0.36) * W, rnd.uniform(0.6, 0.95))
        for _ in range(2)
    ]
    points = []
    for x in range(0, W + 25, 24):
        s = sum(a * math.sin(math.tau * f * x / W + p) for f, p, a in sines)
        t = sum(h * soft_tent(x, c, w) for c, w, h in peaks)
        points.append(f"{x},{base - amp * (0.5 * s + t):.1f}")
    return points


def fill_to_bottom(points):
    """The path under an edge's points, down to the picture's bottom."""
    return f"M0,{H} L" + " L".join(points) + f" L{W},{H} Z"


def ridges(sky, glow, glow_at, near, far, moon):
    """Four mountain ranges, each one smooth ridge line, receding into haze:
    the farthest is nearly the sky. On light, a low sun glow behind them; on
    dark (`moon` a colour), a crescent moon with its halo, and stars."""
    layers = ramp(near, far, 4)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="0.75" stop-color="{sky[1]}"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="glow" gradientUnits="userSpaceOnUse" cx="{glow_at[0] * W:.0f}" '
        f'cy="{glow_at[1] * H:.0f}" r="{0.42 * H:.0f}"><stop offset="0" stop-color="{glow}" '
        f'stop-opacity="0.85"/><stop offset="1" stop-color="{glow}" stop-opacity="0"/>'
        "</radialGradient>"
    )
    for i, colour in enumerate(layers):
        out.append(
            f'<linearGradient id="l{i}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
            f'stop-color="{colour}"/><stop offset="1" stop-color="{mix(colour, near, 0.2)}"/>'
            "</linearGradient>"
        )
    if moon is not None:
        mx, my, r = 0.72 * W, 0.22 * H, 0.04 * H
        out.append(
            f'<radialGradient id="halo" gradientUnits="userSpaceOnUse" cx="{mx:.0f}" '
            f'cy="{my:.0f}" r="{3.4 * r:.0f}"><stop offset="0" stop-color="{moon}" '
            f'stop-opacity="0.32"/><stop offset="1" stop-color="{moon}" stop-opacity="0"/>'
            "</radialGradient>"
        )
        out.append(
            f'<mask id="cres" maskUnits="userSpaceOnUse" x="0" y="0" width="{W}" height="{H}">'
            f'<rect width="{W}" height="{H}" fill="#ffffff"/><circle cx="{mx + 0.3 * r:.0f}" '
            f'cy="{my - 0.12 * r:.0f}" r="{0.92 * r:.0f}" fill="#000000"/></mask>'
        )
    out.append("</defs>")
    out.append(f'<rect width="{W}" height="{H}" fill="url(#sky)"/>')
    out.append(f'<rect width="{W}" height="{H}" fill="url(#glow)"/>')
    if moon is not None:
        out += stars(23, 300, 0.6)
        out.append(f'<circle cx="{mx:.0f}" cy="{my:.0f}" r="{3.4 * r:.0f}" fill="url(#halo)"/>')
        out.append(f'<circle cx="{mx:.0f}" cy="{my:.0f}" r="{r:.0f}" fill="{moon}" mask="url(#cres)"/>')
    for i, (base, amp) in enumerate([(0.58, 0.16), (0.68, 0.14), (0.79, 0.12), (0.90, 0.09)]):
        points = ridge_points(base * H, amp * H, seed=31 + i)
        out.append(f'<path d="{fill_to_bottom(points)}" fill="url(#l{i})"/>')
    out.append("</svg>")
    return "\n".join(out)


def fog(y, seed, colour, alpha):
    """A bank of fog along `y`: four wide ellipses, each fading to nothing at
    its edges (the `fogg` gradient), blurred so no edge shows."""
    rnd = random.Random(seed)
    parts = []
    for _ in range(4):
        cx = rnd.uniform(0.1, 0.9) * W
        rx = rnd.uniform(0.35, 0.6) * W
        ry = rnd.uniform(0.05, 0.08) * H
        cy = y + rnd.uniform(-0.012, 0.012) * H
        parts.append(
            f'<ellipse cx="{cx:.0f}" cy="{cy:.0f}" rx="{rx:.0f}" ry="{ry:.0f}" '
            f'fill="url(#fogg)" opacity="{alpha:.2f}"/>'
        )
    return '<g filter="url(#fog)">' + "".join(parts) + "</g>"


def mist(sky, glow, glow_at, near, far, fog_colour, fog_alpha, starry):
    """A valley at dawn, or under a moon: three low hill layers, with a soft
    bank of fog lying between each pair of them."""
    layers = ramp(near, far, 3)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="0.75" stop-color="{sky[1]}"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="glow" gradientUnits="userSpaceOnUse" cx="{glow_at[0] * W:.0f}" '
        f'cy="{glow_at[1] * H:.0f}" r="{0.5 * H:.0f}"><stop offset="0" stop-color="{glow}" '
        f'stop-opacity="0.8"/><stop offset="1" stop-color="{glow}" stop-opacity="0"/>'
        "</radialGradient>"
    )
    out.append(
        f'<radialGradient id="fogg"><stop offset="0" stop-color="{fog_colour}" stop-opacity="1"/>'
        f'<stop offset="0.5" stop-color="{fog_colour}" stop-opacity="0.6"/>'
        f'<stop offset="1" stop-color="{fog_colour}" stop-opacity="0"/></radialGradient>'
    )
    for i, colour in enumerate(layers):
        out.append(
            f'<linearGradient id="l{i}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
            f'stop-color="{colour}"/><stop offset="1" stop-color="{mix(colour, near, 0.15)}"/>'
            "</linearGradient>"
        )
    out.append(
        '<filter id="fog" x="-30%" y="-100%" width="160%" height="300%">'
        '<feGaussianBlur stdDeviation="60"/></filter>'
        '<filter id="shade" x="-5%" y="-10%" width="110%" height="120%">'
        '<feGaussianBlur stdDeviation="40"/></filter></defs>'
    )
    out.append(f'<rect width="{W}" height="{H}" fill="url(#sky)"/>')
    out.append(f'<rect width="{W}" height="{H}" fill="url(#glow)"/>')
    if starry:
        out += stars(41, 180, 0.45)
    for i, (base, amp, seed) in enumerate([(0.60, 0.04, 41), (0.70, 0.05, 42), (0.84, 0.04, 43)]):
        if i == 1:
            out.append(fog(0.655 * H, 51, fog_colour, fog_alpha))
        if i == 2:
            out.append(fog(0.775 * H, 52, fog_colour, fog_alpha))
        d = wave(base * H, amp * H, seed)
        out.append(
            f'<path d="{d}" fill="{layers[i]}" opacity="0.5" '
            'transform="translate(0,-18)" filter="url(#shade)"/>'
        )
        out.append(f'<path d="{d}" fill="url(#l{i})"/>')
    out.append("</svg>")
    return "\n".join(out)


def arc_ribbon(thick, phase, shift=0.0):
    """One wide band of a circle over the valley, thinner where it fades at
    its ends and thickest in the middle; `shift` moves it inward or outward."""
    cx, cy, radius, reach = W / 2, 1.12 * H, 0.94 * H + shift * H, 0.95
    steps = 180
    inner, outer = [], []
    for k in range(steps + 1):
        u = k / steps
        phi = math.pi / 2 + reach * (1 - 2 * u)
        r = radius + 0.012 * H * math.sin(math.tau * 0.9 * u + phase)
        t = thick * H * (0.55 + 0.45 * math.sin(math.pi * u + phase))
        for radius_at, points in ((r + t / 2, outer), (r - t / 2, inner)):
            points.append(
                (cx + radius_at * math.cos(phi), cy - radius_at * math.sin(phi))
            )
    return "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in outer + inner[::-1]) + " Z"


def arc(sky, colours, silhouettes, starry):
    """One wide arc of soft light over a horizon, with a faint second arc
    inside it and a low landscape at the bottom: the glow only, no line."""
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0.3" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="1" stop-color="{sky[1]}"/></linearGradient>'
    )
    a, b, c = colours
    out.append(
        '<linearGradient id="arc" x1="0" y1="0" x2="1" y2="0">'
        f'<stop offset="0" stop-color="{a}" stop-opacity="0"/><stop offset="0.3" stop-color="{a}"/>'
        f'<stop offset="0.6" stop-color="{b}"/><stop offset="0.9" stop-color="{c}"/>'
        f'<stop offset="1" stop-color="{c}" stop-opacity="0"/></linearGradient>'
    )
    for name, blur in (("wide", 160), ("mid", 70), ("faint", 120), ("ground", 8)):
        out.append(
            f'<filter id="{name}" x="-20%" y="-150%" width="140%" height="400%">'
            f'<feGaussianBlur stdDeviation="{blur}"/></filter>'
        )
    out.append("</defs>")
    out.append(f'<rect width="{W}" height="{H}" fill="url(#sky)"/>')
    if starry:
        out += stars(11, 320, 0.7)
    out.append(f'<path d="{arc_ribbon(0.2, 0.6)}" fill="url(#arc)" opacity="0.7" filter="url(#wide)"/>')
    out.append(f'<path d="{arc_ribbon(0.07, 0.6)}" fill="url(#arc)" opacity="0.8" filter="url(#mid)"/>')
    out.append(
        f'<path d="{arc_ribbon(0.12, 2.0, shift=-0.1)}" fill="url(#arc)" opacity="0.3" filter="url(#faint)"/>'
    )
    out.append('<g filter="url(#ground)">')
    for base, amp, seed, colour in silhouettes:
        out.append(f'<path d="{fill_to_bottom(wave_points(base * H, amp * H, seed))}" fill="{colour}"/>')
    out.append("</g>")
    out.append("</svg>")
    return "\n".join(out)


def wave_points(base, amp, seed):
    """The top edge of a hill, as points, from the same sines as `wave`."""
    rnd = random.Random(seed)
    parts = [
        (rnd.uniform(0.6, 1.6) * k, rnd.uniform(0, math.tau), amp / (k * 1.3))
        for k in (1, 2.3, 4.1)
    ]
    points = []
    for x in range(0, W + 41, 40):
        y = base + sum(a * math.sin(math.tau * f * x / W + p) for f, p, a in parts)
        points.append(f"{x},{y:.1f}")
    return points


def ocean(sky, sun, sun_at, sea_far, sea_near, glitter):
    """The horizon a third down: the sky with a soft sun or moon glow, and
    the sea as six soft horizontal bands of water, each lighter towards the
    horizon, with a glitter path of blurred ellipses under the sun."""
    horizon = H / 3
    n = 6
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="1" stop-color="{sky[1]}"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="sun" gradientUnits="userSpaceOnUse" cx="{sun_at[0] * W:.0f}" '
        f'cy="{sun_at[1] * H:.0f}" r="{0.5 * H:.0f}"><stop offset="0" stop-color="{sun}" '
        f'stop-opacity="0.9"/><stop offset="1" stop-color="{sun}" stop-opacity="0"/>'
        "</radialGradient>"
    )
    out.append(
        '<filter id="swell" x="-5%" y="-10%" width="110%" height="120%">'
        '<feGaussianBlur stdDeviation="7"/></filter>'
        '<filter id="glint" x="-50%" y="-50%" width="200%" height="200%">'
        '<feGaussianBlur stdDeviation="24"/></filter></defs>'
    )
    out.append(f'<rect width="{W}" height="{horizon:.0f}" fill="url(#sky)"/>')
    out.append(f'<rect width="{W}" height="{horizon:.0f}" fill="url(#sun)"/>')
    out.append('<g filter="url(#swell)">')
    for k in range(n):
        frac = (k / n) ** 1.5
        base = horizon + 0.9 * (H - horizon) * frac
        amp = 3 + 10 * frac
        points = sea_edge(base, amp, seed=71 + k, swell=1 + 1.2 * frac)
        t = k / (n - 1)
        out.append(f'<path d="{fill_to_bottom(points)}" fill="{mix(sea_near, sea_far, 1 - t)}"/>')
    out.append("</g>")
    rnd = random.Random(81)
    glints = []
    for _ in range(30):
        frac = rnd.uniform(0.02, 1) ** 1.2
        y = horizon + 0.9 * (H - horizon) * frac
        x = sun_at[0] * W + rnd.gauss(0, 30 + 200 * frac)
        rx = (60 + 240 * frac) * rnd.uniform(0.7, 1.2)
        ry = 12 + 34 * frac
        opacity = (0.4 * (1 - frac) + 0.08) * rnd.uniform(0.6, 1)
        glints.append(
            f'<ellipse cx="{x:.0f}" cy="{y:.0f}" rx="{rx:.0f}" ry="{ry:.1f}" '
            f'fill="{glitter}" opacity="{opacity:.2f}"/>'
        )
    out.append('<g filter="url(#glint)">' + "".join(glints) + "</g>")
    out.append("</svg>")
    return "\n".join(out)


def sea_edge(base, amp, seed, swell=1.0):
    """The top edge of a band of water, as points: a gentle sum of sines,
    stronger with `swell` for the bands nearer the viewer."""
    rnd = random.Random(seed)
    parts = [
        (rnd.uniform(0.7, 1.3) * k * swell, rnd.uniform(0, math.tau), amp / k)
        for k in (1, 1.9, 3.3)
    ]
    points = []
    for x in range(0, W + 19, 18):
        y = base + sum(a * math.sin(math.tau * f * x / W + p) for f, p, a in parts)
        points.append(f"{x},{y:.1f}")
    return points


def calm(sky, horizon_colour, sun, sun_at, water_near, water_deep, streak, starry):
    """Still water under the sky, the horizon at the middle: the sky's glow
    mirrored in the water, and a few long low streaks lying on it."""
    horizon = H / 2
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{sky[0]}"/><stop offset="1" stop-color="{horizon_colour}"/></linearGradient>'
    )
    out.append(
        f'<linearGradient id="water" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{water_near}"/><stop offset="1" stop-color="{water_deep}"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="sun" gradientUnits="userSpaceOnUse" cx="{sun_at[0] * W:.0f}" '
        f'cy="{sun_at[1] * H:.0f}" r="{0.36 * H:.0f}"><stop offset="0" stop-color="{sun}" '
        f'stop-opacity="0.9"/><stop offset="1" stop-color="{sun}" stop-opacity="0"/>'
        "</radialGradient>"
    )
    out.append(
        f'<radialGradient id="mirror" gradientUnits="userSpaceOnUse" cx="{sun_at[0] * W:.0f}" '
        f'cy="{horizon + 0.04 * H:.0f}" r="{0.2 * H:.0f}"><stop offset="0" stop-color="{sun}" '
        f'stop-opacity="0.55"/><stop offset="1" stop-color="{sun}" stop-opacity="0"/>'
        "</radialGradient>"
    )
    out.append(
        '<filter id="streak" x="-20%" y="-200%" width="140%" height="500%">'
        '<feGaussianBlur stdDeviation="7"/></filter></defs>'
    )
    out.append(f'<rect width="{W}" height="{horizon:.0f}" fill="url(#sky)"/>')
    out.append(f'<rect width="{W}" height="{horizon:.0f}" fill="url(#sun)"/>')
    if starry:
        out += stars(17, 200, 0.45)
    out.append(f'<rect y="{horizon:.0f}" width="{W}" height="{H - horizon:.0f}" fill="url(#water)"/>')
    out.append(f'<rect y="{horizon:.0f}" width="{W}" height="{H - horizon:.0f}" fill="url(#mirror)"/>')
    rnd = random.Random(91)
    shapes = []
    for _ in range(26):
        frac = rnd.uniform(0.02, 1) ** 1.3
        y = horizon + 0.04 * H + (H - horizon - 0.04 * H) * frac
        cx = rnd.uniform(0, W)
        rx = rnd.uniform(0.08, 0.34) * W * (0.5 + frac)
        ry = (2 + 8 * frac) * rnd.uniform(0.6, 1.2)
        shapes.append(
            f'<ellipse cx="{cx:.0f}" cy="{y:.0f}" rx="{rx:.0f}" ry="{ry:.1f}" '
            f'fill="{streak}" opacity="{rnd.uniform(0.08, 0.35):.2f}"/>'
        )
    out.append('<g filter="url(#streak)">' + "".join(shapes) + "</g>")
    out.append("</svg>")
    return "\n".join(out)


def deep(top, bottom, ray, rays_at, seed_stars):
    """Seen from under the surface: light coming down through the water
    from the top, fanning out in soft rays into deep blue, with specks."""
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}"><defs>']
    out.append(
        f'<linearGradient id="water" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient>'
    )
    out.append(
        f'<linearGradient id="ray" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
        f'stop-color="{ray}" stop-opacity="0.9"/><stop offset="0.6" stop-color="{ray}" '
        f'stop-opacity="0.25"/><stop offset="1" stop-color="{ray}" stop-opacity="0"/></linearGradient>'
    )
    out.append(
        f'<radialGradient id="source" gradientUnits="userSpaceOnUse" cx="{rays_at[0] * W:.0f}" '
        f'cy="{rays_at[1] * H:.0f}" r="{0.6 * H:.0f}"><stop offset="0" stop-color="{ray}" '
        f'stop-opacity="0.45"/><stop offset="1" stop-color="{ray}" stop-opacity="0"/></radialGradient>'
    )
    out.append(
        '<filter id="rays" x="-20%" y="-20%" width="140%" height="140%">'
        '<feGaussianBlur stdDeviation="26"/></filter></defs>'
    )
    out.append(f'<rect width="{W}" height="{H}" fill="url(#water)"/>')
    out.append(f'<rect width="{W}" height="{H}" fill="url(#source)"/>')
    rnd = random.Random(101)
    sx, sy = rays_at[0] * W, rays_at[1] * H
    rays = []
    for _ in range(13):
        angle = rnd.uniform(-0.62, 0.62)
        bottom_y = H * 1.05
        bx = sx + math.tan(angle) * (bottom_y - sy)
        half = rnd.uniform(0.012, 0.075) * W
        opacity = rnd.uniform(0.25, 0.7)
        rays.append(
            f'<polygon points="{sx - 8:.0f},{sy:.0f} {sx + 8:.0f},{sy:.0f} '
            f'{bx + half:.0f},{bottom_y:.0f} {bx - half:.0f},{bottom_y:.0f}" '
            f'fill="url(#ray)" opacity="{opacity:.2f}"/>'
        )
    out.append('<g filter="url(#rays)">' + "".join(rays) + "</g>")
    out += stars(seed_stars, 160, 1.0)
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
            near=mix(mix(accent_light, "#2a3570", 0.25), "#ffffff", 0.15),
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
        **collections(),
    }


def collections():
    """The pictures of the built-in themes' collections (M5.12b6d): Edel's
    ridges and mist, Aurora's arc, and Ocean's three. Each a light
    and a dark picture, from the same tokens as the first pictures."""
    dark, light = tokens()
    night = "#0b1024"
    dawn = "#fbe3d6"
    accent, accent_light = dark["accent"], light["accent"]
    navy = dark["lock_backdrop_deep"]
    glow_dark = dark["lock_glow_second"]
    violet = "#6a5aa8"
    sky_dark = (mix(navy, night, 0.5), mix(glow_dark, violet, 0.2))
    sky_light = (mix(accent_light, "#ffffff", 0.82), dawn)
    far_light = mix(accent_light, "#ffffff", 0.78)
    near_light = mix(mix(accent_light, "#2a3570", 0.25), "#ffffff", 0.15)
    far_dark = mix(dark["lock_backdrop"], accent, 0.1)
    near_dark = mix(navy, night, 0.7)
    aurora_colours_dark = ("#38c8b4", "#7a5bd0", accent, "#c06bd0")
    aurora_colours_light = ("#5fd0bd", "#a98cff", mix(accent_light, "#ffffff", 0.45), "#e48fd0")
    aurora_sky_light = (mix(accent_light, "#ffffff", 0.92), mix(accent_light, "#ffffff", 0.84))
    sea_far_dark = mix(SEA_MID, "#2a3f6e", 0.3)
    sea_near_dark = mix(SEA_DEEP, night, 0.3)
    sea_far_light = mix(SEA_TEAL, "#ffffff", 0.45)
    sea_near_light = mix(SEA_MID, accent_light, 0.4)
    return {
        # Edel: the release's family, sharper and hazier than Hills.
        "ridges-light": ridges(
            sky=sky_light,
            glow="#ffc9a8",
            glow_at=(0.30, 0.66),
            near=near_light,
            far=mix(sky_light[1], "#ffffff", 0.35),
            moon=None,
        ),
        "ridges-dark": ridges(
            sky=sky_dark,
            glow=mix(glow_dark, "#ffffff", 0.3),
            glow_at=(0.72, 0.22),
            near=near_dark,
            far=mix(sky_dark[1], "#8c8ccc", 0.22),
            moon="#e6eaff",
        ),
        "mist-light": mist(
            sky=sky_light,
            glow="#ffe0c8",
            glow_at=(0.5, 0.6),
            near=near_light,
            far=far_light,
            fog_colour="#ffffff",
            fog_alpha=0.6,
            starry=False,
        ),
        "mist-dark": mist(
            sky=sky_dark,
            glow=mix(glow_dark, "#c8d4f0", 0.5),
            glow_at=(0.25, 0.3),
            near=near_dark,
            far=far_dark,
            fog_colour=mix(dark["lock_glow"], "#c8d4f0", 0.5),
            fog_alpha=0.4,
            starry=True,
        ),
        # Aurora: an arc of light over a horizon, and curtains of light.
        "aurora-arc-light": arc(
            sky=aurora_sky_light,
            colours=(aurora_colours_light[0], aurora_colours_light[1], aurora_colours_light[2]),
            silhouettes=[
                (0.82, 0.04, 21, far_light),
                (0.91, 0.04, 22, near_light),
            ],
            starry=False,
        ),
        "aurora-arc-dark": arc(
            sky=(night, navy),
            colours=(aurora_colours_dark[0], aurora_colours_dark[1], aurora_colours_dark[2]),
            silhouettes=[
                (0.80, 0.02, 21, mix(navy, night, 0.55)),
                (0.90, 0.025, 22, mix(night, "#000000", 0.35)),
            ],
            starry=True,
        ),
        # Ocean: the horizon a third down, still water, and the sea from under.
        "ocean-light": ocean(
            sky=(mix(accent_light, "#ffffff", 0.85), "#fbe3d6"),
            sun="#ffe2c4",
            sun_at=(0.68, 0.22),
            sea_far=sea_far_light,
            sea_near=sea_near_light,
            glitter="#ffffff",
        ),
        "ocean-dark": ocean(
            sky=(night, sky_dark[1]),
            sun=mix(glow_dark, "#ffffff", 0.5),
            sun_at=(0.30, 0.22),
            sea_far=sea_far_dark,
            sea_near=sea_near_dark,
            glitter="#a9d8ff",
        ),
        "ocean-calm-light": calm(
            sky=(mix(accent_light, "#ffffff", 0.85), dawn),
            horizon_colour=dawn,
            sun="#ffd9bf",
            sun_at=(0.68, 0.5),
            water_near=mix(dawn, "#ffffff", 0.35),
            water_deep=mix(SEA_TEAL, "#ffffff", 0.45),
            streak="#ffffff",
            starry=False,
        ),
        "ocean-calm-dark": calm(
            sky=(night, sky_dark[1]),
            horizon_colour=sky_dark[1],
            sun=mix(glow_dark, "#ffffff", 0.45),
            sun_at=(0.68, 0.5),
            water_near=sky_dark[1],
            water_deep=mix(SEA_DEEP, night, 0.4),
            streak="#c8d4f0",
            starry=True,
        ),
        "ocean-deep-light": deep(
            top=mix(SEA_TEAL, "#ffffff", 0.6),
            bottom=mix(SEA_TEAL, "#ffffff", 0.2),
            ray="#ffffff",
            rays_at=(0.55, -0.04),
            seed_stars=103,
        ),
        "ocean-deep-dark": deep(
            top=SEA_MID,
            bottom=mix(SEA_DEEP, night, 0.5),
            ray="#7fd8ff",
            rays_at=(0.55, -0.04),
            seed_stars=103,
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
    # A faint grain, so the soft gradients show no bands in 8 bits.
    run(
        "convert", str(png_path), "-seed", "1", "-attenuate", str(GRAIN), "+noise", "Uniform",
        "-quality", str(QUALITY), str(out / f"{name}.jpg"),
    )
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
