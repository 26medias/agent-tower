#!/usr/bin/env python3
# Raw PTY driver for probes. It acts like the app: one PTY master, raw byte writes.
#
# Usage: ptydrive.py <name> <cols> <rows> -- <command...>
# Files: /tmp/agent-tower-probes/pty/<name>.out  raw output of the CLI (append)
#        /tmp/agent-tower-probes/pty/<name>.ctl  FIFO; each line is a JSON string of bytes
#                                                to write in ONE os.write (latin-1 code points)
# It answers terminal queries that TUIs send at start (DSR cursor position, DA1),
# because there is no real terminal emulator in front of it.
import fcntl
import json
import os
import pty
import select
import struct
import sys
import termios

name, cols, rows = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
cmd = sys.argv[sys.argv.index("--") + 1:]
base = "/tmp/agent-tower-probes/pty"
os.makedirs(base, exist_ok=True)
outp, ctlp = f"{base}/{name}.out", f"{base}/{name}.ctl"
if os.path.exists(ctlp):
    os.unlink(ctlp)
os.mkfifo(ctlp)

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.execvp(cmd[0], cmd)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
ctl = os.open(ctlp, os.O_RDONLY | os.O_NONBLOCK)
out = open(outp, "ab", buffering=0)
buf = b""
while True:
    try:
        r, _, _ = select.select([fd, ctl], [], [], 0.5)
    except InterruptedError:
        continue
    if fd in r:
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        out.write(data)
        # Minimal terminal replies.
        if b"\x1b[6n" in data:
            os.write(fd, b"\x1b[1;1R")
        if b"\x1b[c" in data or b"\x1b[0c" in data:
            os.write(fd, b"\x1b[?62;22c")
    if ctl in r:
        chunk = os.read(ctl, 65536)
        if not chunk:
            os.close(ctl)
            ctl = os.open(ctlp, os.O_RDONLY | os.O_NONBLOCK)
            continue
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            if line.strip():
                payload = json.loads(line).encode("latin-1")
                os.write(fd, payload)
                out.write(b"")
_, status = os.waitpid(pid, 0)
print("exit", status)
