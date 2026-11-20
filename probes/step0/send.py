#!/usr/bin/env python3
# Usage: send.py <name> <python string literal>   e.g. send.py c2 "'hello\r'"
# Writes the bytes to the driver FIFO; the driver writes them in ONE os.write.
import ast, json, sys
s = ast.literal_eval(sys.argv[2])
with open(f"/tmp/agent-tower-step0/pty/{sys.argv[1]}.ctl", "w") as f:
    f.write(json.dumps(s) + "\n")
