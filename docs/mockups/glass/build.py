#!/usr/bin/env python3
"""Builds review.html, the design review's boards, from boards.html: each
{{icon:NAME}} becomes design/icons/NAME.svg inline, drawn in currentColor,
and {{wall:NAME}} the wallpaper's file URL. Run from the repository root;
then `node render.js` here writes one JPEG per board."""
import os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
repo = sys.argv[1] if len(sys.argv) > 1 else os.getcwd()
walls = os.path.join(repo, "features/wallpapers/usr/share/edel/wallpapers")


def icon(m):
    name, size = m.group(1), m.group(2) or "16"
    svg = open(os.path.join(repo, "design/icons", name + ".svg")).read()
    svg = re.sub(r"<!--.*?-->", "", svg, flags=re.S)
    svg = svg.replace('stroke="#000"', 'stroke="currentColor"').replace('fill="#000"', 'fill="currentColor"')
    return svg.replace("<svg ", f'<svg width="{size}" height="{size}" class="i" ', 1)


src = open(os.path.join(here, "boards.html")).read()
src = re.sub(r"\{\{icon:([a-z0-9-]+)(?::(\d+))?\}\}", icon, src)
src = re.sub(r"\{\{wall:([a-z-]+)\}\}", lambda m: "file://" + os.path.join(walls, m.group(1) + ".jpg"), src)
open(os.path.join(here, "review.html"), "w").write(src)
print("wrote review.html")
