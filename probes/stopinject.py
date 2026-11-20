#!/usr/bin/env python3
# Stop hook for the delivery probe. It logs the event; if a message waits in
# inbox/<session_id>.txt it removes the file and returns decision=block so the CLI
# continues the turn with the message as the reason. Works for Claude and Codex payloads.
import json, os, sys, time
raw = sys.stdin.read()
d = json.loads(raw) if raw.strip() else {}
sid = d.get("session_id", "")
p = f"/tmp/agent-tower-probes/inbox/{sid}.txt"
with open(sys.argv[1], "a") as f:
    f.write(json.dumps({"t": round(time.time(), 3), "label": "StopInject", "data": d, "env": {}}) + "\n")
if os.path.exists(p):
    msg = open(p).read()
    os.unlink(p)
    print(json.dumps({"decision": "block", "reason": msg}))
sys.exit(0)
