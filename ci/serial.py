#!/usr/bin/env python3
"""Types answers into a VM's serial port (roadmap M6.6b), with the
Python standard library only.

    serial.py SOCKET WAIT=ANSWER ...

Connects to SOCKET, the unix socket QEMU serves a serial port on
(`-chardev socket,...,server=on`), retrying until it is there. For each
pair in order it waits until the port has printed WAIT, then types
ANSWER and Return; an ANSWER of `@N TEXT` waits N seconds first, as a
slow person would (M8.2). What the port printed goes to standard output, so a
test can keep it and check that no password was echoed. Exits 1 when a
WAIT has not come within the time limit (SERIAL_TIMEOUT, 900 s).
"""

import os
import socket
import sys
import time


def main(argv):
    path, pairs = argv[1], [a.split("=", 1) for a in argv[2:]]
    deadline = time.monotonic() + int(os.environ.get("SERIAL_TIMEOUT", "900"))
    sock = socket.socket(socket.AF_UNIX)
    while True:
        try:
            sock.connect(path)
            break
        except OSError:
            if time.monotonic() > deadline:
                sys.exit(f"serial: {path} never opened")
            time.sleep(1)
    sock.settimeout(1)
    seen = b""
    for wait, answer in pairs:
        while wait.encode() not in seen:
            if time.monotonic() > deadline:
                sys.stdout.flush()
                sys.exit(f"serial: {wait!r} never came")
            try:
                data = sock.recv(4096)
            except socket.timeout:
                continue
            if not data:
                sys.exit(f"serial: the port closed before {wait!r}")
            sys.stdout.buffer.write(data)
            sys.stdout.flush()
            seen += data
        seen = seen[seen.index(wait.encode()) + len(wait) :]
        # The prompt is out; give the program a moment to read.
        time.sleep(0.5)
        if answer.startswith("@"):
            pause, answer = answer[1:].split(" ", 1)
            time.sleep(int(pause))
        sock.sendall(answer.encode() + b"\r")
    # What the program says after the last answer, until it goes quiet.
    quiet = time.monotonic() + 30
    while time.monotonic() < min(quiet, deadline):
        try:
            data = sock.recv(4096)
        except socket.timeout:
            continue
        if not data:
            break
        sys.stdout.buffer.write(data)
        sys.stdout.flush()
        quiet = time.monotonic() + 30


if __name__ == "__main__":
    main(sys.argv)
