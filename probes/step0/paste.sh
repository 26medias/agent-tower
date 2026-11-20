#!/bin/sh
# paste.sh <name> <text> : bracketed paste, then Enter 0.5 s later (like the app would)
cd /tmp/agent-tower-step0
python3 - "$1" "$2" <<'PY'
import sys,json
n,t=sys.argv[1],sys.argv[2]
open(f"/tmp/agent-tower-step0/pty/{n}.ctl","w").write(json.dumps("\x1b[200~"+t+"\x1b[201~")+"\n")
PY
sleep 0.5
./send.py "$1" "'\r'"
