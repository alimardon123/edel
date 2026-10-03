#!/usr/bin/env python3
"""Drives a QEMU VM's screen, keyboard and tablet for the desktop tests
(roadmap M4.1), with the Python standard library only.

    qmp.py screendump FILE.png   save the screen as PNG (QEMU 7.1 or newer)
    qmp.py click X Y             move the pointer to pixel X, Y and click
    qmp.py key KEY...            press each chord, such as ctrl-alt-t or ret
    qmp.py type TEXT             type TEXT; \\n is Return
    qmp.py pixel FILE.png X Y    print the colour at X, Y as rrggbb
    qmp.py size FILE.png         print WIDTH HEIGHT
    qmp.py uniform FILE.png      print "uniform rrggbb" or "varied"

The first three talk to the QMP socket named by $QMP. Key names are
QEMU's QKeyCodes (a, 1, ret, spc, ctrl, alt, meta_l, f1 and so on).
"""

import json
import os
import socket
import struct
import sys
import tempfile
import time
import zlib

# QEMU scales absolute pointer positions to 0 to 0x7fff.
ABS_MAX = 0x7FFF

# Characters `type` sends with shift held, and the key that makes each.
SHIFTED = {
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7",
    "*": "8", "(": "9", ")": "0", "_": "minus", "+": "equal", "{": "bracket_left",
    "}": "bracket_right", "|": "backslash", ":": "semicolon", '"': "apostrophe",
    "<": "comma", ">": "dot", "?": "slash", "~": "grave_accent",
}
PLAIN = {
    " ": "spc", "\n": "ret", "\t": "tab", "-": "minus", "=": "equal",
    "[": "bracket_left", "]": "bracket_right", "\\": "backslash", ";": "semicolon",
    "'": "apostrophe", ",": "comma", ".": "dot", "/": "slash", "`": "grave_accent",
}


class Qmp:
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.file = self.sock.makefile("rwb")
        self.read()  # the greeting
        self.run("qmp_capabilities")

    def read(self):
        line = self.file.readline()
        if not line:
            raise SystemExit("qmp: QEMU closed the socket")
        return json.loads(line)

    def run(self, command, **arguments):
        message = {"execute": command}
        if arguments:
            message["arguments"] = arguments
        self.file.write(json.dumps(message).encode() + b"\n")
        self.file.flush()
        while True:
            reply = self.read()
            if "event" in reply:
                continue
            if "error" in reply:
                raise SystemExit(f"qmp: {command}: {reply['error'].get('desc', reply['error'])}")
            return reply.get("return")


def keys(qmp, chords):
    for chord in chords:
        names = chord.split("-")
        qmp.run("send-key", keys=[{"type": "qcode", "data": n} for n in names])
        time.sleep(0.05)


def type_text(qmp, text):
    chords = []
    for c in text:
        if c.isascii() and (c.islower() or c.isdigit()):
            chords.append(c)
        elif c.isascii() and c.isupper():
            chords.append("shift-" + c.lower())
        elif c in PLAIN:
            chords.append(PLAIN[c])
        elif c in SHIFTED:
            chords.append("shift-" + SHIFTED[c])
        else:
            raise SystemExit(f"qmp: cannot type {c!r}")
    keys(qmp, chords)


def screendump(qmp, path):
    path = os.path.abspath(path)
    qmp.run("screendump", filename=path, format="png")
    # QEMU writes the file before it replies; wait for a complete PNG anyway.
    for _ in range(50):
        if os.path.exists(path) and os.path.getsize(path) > 0:
            return
        time.sleep(0.1)
    raise SystemExit(f"qmp: screendump wrote nothing to {path}")


def click(qmp, x, y):
    with tempfile.TemporaryDirectory() as tmp:
        shot = os.path.join(tmp, "size.png")
        screendump(qmp, shot)
        width, height = png_size(shot)
    events = [
        {"type": "abs", "data": {"axis": "x", "value": x * ABS_MAX // max(width - 1, 1)}},
        {"type": "abs", "data": {"axis": "y", "value": y * ABS_MAX // max(height - 1, 1)}},
    ]
    qmp.run("input-send-event", events=events)
    for down in (True, False):
        time.sleep(0.05)
        qmp.run("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": "left"}}])


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n" or head[12:16] != b"IHDR":
        raise SystemExit(f"{path}: not a PNG")
    return struct.unpack(">II", head[16:24])


def read_png(path):
    """Returns width, height and rows of (r, g, b) tuples for an 8-bit RGB
    or RGBA PNG without interlacing, which is what QEMU writes."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos, idat, header = 8, b"", None
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    width, height, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise SystemExit(f"{path}: only 8-bit RGB or RGBA PNGs without interlacing")
    bpp = 3 if colour == 2 else 4
    raw = zlib.decompress(idat)
    stride = width * bpp
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if kind == 1:
                line[i] = (line[i] + a) & 0xFF
            elif kind == 2:
                line[i] = (line[i] + b) & 0xFF
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        rows.append([tuple(line[x * bpp:x * bpp + 3]) for x in range(width)])
        prev = line
    return width, height, rows


def hex_colour(rgb):
    return "%02x%02x%02x" % rgb


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    command, args = argv[1], argv[2:]
    if command in ("pixel", "size", "uniform"):
        width, height, rows = read_png(args[0])
        if command == "size":
            print(width, height)
        elif command == "pixel":
            x, y = int(args[1]), int(args[2])
            if not (0 <= x < width and 0 <= y < height):
                raise SystemExit(f"{args[0]}: {x}, {y} is outside {width}x{height}")
            print(hex_colour(rows[y][x]))
        else:
            colours = {p for row in rows for p in row}
            print("uniform " + hex_colour(colours.pop()) if len(colours) == 1 else "varied")
        return
    qmp = Qmp(os.environ["QMP"])
    if command == "screendump":
        screendump(qmp, args[0])
    elif command == "click":
        click(qmp, int(args[0]), int(args[1]))
    elif command == "key":
        keys(qmp, args)
    elif command == "type":
        type_text(qmp, args[0].replace("\\n", "\n"))
    else:
        raise SystemExit(f"qmp: unknown command {command}\n{__doc__}")


if __name__ == "__main__":
    main(sys.argv)
