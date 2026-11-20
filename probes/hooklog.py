#!/usr/bin/env python3
# Hook logger for probes. Usage: hooklog.py <logfile> <label>
# Reads the hook JSON from stdin and appends one line to <logfile>.
import json
import os
import sys
import time

log = sys.argv[1]
label = sys.argv[2] if len(sys.argv) > 2 else ""
raw = sys.stdin.read()
try:
    data = json.loads(raw) if raw.strip() else None
except Exception:
    data = {"_unparsed": raw[:2000]}
env = {k: v for k, v in os.environ.items() if k.startswith(("CLAUDE", "CODEX", "AT_"))
       and "TOKEN" not in k and "KEY" not in k}
with open(log, "a") as f:
    f.write(json.dumps({"t": round(time.time(), 3), "label": label, "argv": sys.argv[3:],
                        "data": data, "env": env}) + "\n")
sys.exit(0)
