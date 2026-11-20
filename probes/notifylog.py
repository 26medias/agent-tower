#!/usr/bin/env python3
# Codex legacy notify program: Codex passes one JSON argument. Append it to the log.
import json, os, sys, time
with open("/tmp/agent-tower-probes/logs/codex-notify.jsonl", "a") as f:
    f.write(json.dumps({"t": round(time.time(), 3), "argv": sys.argv[1:], "AT_EMP": os.environ.get("AT_EMP"), "CODEX_THREAD_ID": os.environ.get("CODEX_THREAD_ID")}) + "\n")
