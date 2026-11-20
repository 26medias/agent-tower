#!/usr/bin/env python3
# Print hook log lines compactly. Usage: show.py <logfile> [maxlen]
import json
import sys

maxlen = int(sys.argv[2]) if len(sys.argv) > 2 else 700
t0 = None
for line in open(sys.argv[1]):
    r = json.loads(line)
    t0 = t0 or r["t"]
    d = r.get("data") or {}
    if isinstance(d, dict):
        d = {k: v for k, v in d.items() if k not in ("transcript_path", "cwd")} | (
            {"transcript_path": "..." + str(d.get("transcript_path"))[-60:]} if d.get("transcript_path") else {})
    s = json.dumps(d)
    print(f"+{r['t'] - t0:7.2f} {r['label']:18} {s[:maxlen]}")
