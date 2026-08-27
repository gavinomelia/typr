#!/usr/bin/env python3
"""Drive an interactive terminal program through a real pty.

Used to smoke-test typr end to end: it allocates a pty with a known window
size, sends keystrokes with delays between them, and prints the final screen
with the escape sequences resolved.

    python3 test/support/drive.py -- ./typr -w 3

Arguments after `--` are the command. Keystrokes are given with --send, and
pauses with --wait, in the order they should happen:

    --send 'the ' --wait 1 --send 'quick ' --wait 1 --send $'\\x1b'
"""

import argparse
import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time


def set_winsize(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def drive(command, steps, rows, cols, settle):
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(command[0], command)

    set_winsize(fd, rows, cols)
    os.set_blocking(fd, False)
    output = bytearray()
    alive = True

    def pump(seconds):
        nonlocal alive
        deadline = time.time() + seconds
        while time.time() < deadline:
            readable, _, _ = select.select([fd], [], [], 0.05)
            if not readable:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                alive = False
                return
            if not chunk:
                alive = False
                return
            output.extend(chunk)

    pump(settle)
    for kind, value in steps:
        if kind == "wait":
            pump(float(value))
        else:
            os.write(fd, value.encode())
            pump(0.25)
    pump(settle)

    try:
        os.kill(pid, 9)
        os.waitpid(pid, 0)
    except OSError:
        pass

    return bytes(output)


def render(data, rows, cols):
    """Replay the escape sequences onto a character grid."""
    text = data.decode("utf8", "replace")
    grid = [[" "] * cols for _ in range(rows)]
    row = col = 0

    i = 0
    while i < len(text):
        char = text[i]
        if char == "\x1b":
            match = re.match(r"\x1b\[([0-9;?]*) ?([A-Za-z])", text[i:])
            if not match:
                i += 1
                continue
            params, final = match.groups()
            if final == "H":
                parts = params.split(";")
                row = int(parts[0] or 1) - 1
                col = int(parts[1]) - 1 if len(parts) > 1 else 0
            elif final == "J":
                grid = [[" "] * cols for _ in range(rows)]
                row = col = 0
            i += match.end()
            continue
        if char == "\r":
            col = 0
        elif char == "\n":
            row, col = row + 1, 0
        elif 0 <= row < rows and 0 <= col < cols:
            grid[row][col] = char
            col += 1
        i += 1

    return "\n".join("".join(line).rstrip() for line in grid)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--send", action="append", default=[])
    parser.add_argument("--wait", action="append", default=[])
    parser.add_argument("--rows", type=int, default=30)
    parser.add_argument("--cols", type=int, default=100)
    parser.add_argument("--settle", type=float, default=1.5)
    parser.add_argument("--raw", action="store_true", help="dump raw bytes instead of the screen")
    parser.add_argument("command", nargs=argparse.REMAINDER)

    args = parser.parse_args()

    # Interleave --send and --wait in the order they appeared on the command line.
    order = []
    sends = list(args.send)
    waits = list(args.wait)
    for argument in sys.argv[1:]:
        if argument == "--send" and sends:
            order.append(("send", sends.pop(0)))
        elif argument == "--wait" and waits:
            order.append(("wait", waits.pop(0)))

    command = args.command
    if command and command[0] == "--":
        command = command[1:]

    data = drive(command, order, args.rows, args.cols, args.settle)
    if args.raw:
        sys.stdout.write(repr(data))
    else:
        sys.stdout.write(render(data, args.rows, args.cols))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
