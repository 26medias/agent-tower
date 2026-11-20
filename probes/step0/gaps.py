#!/usr/bin/env python3
# gaps.py <name> <t_from> [t_to] : prints SEND events and the PTY output events after t_from, with gap to previous output
import sys
n=sys.argv[1]; a=float(sys.argv[2]); b=float(sys.argv[3]) if len(sys.argv)>3 else 1e12
prev=None
for l in open(f'/tmp/agent-tower-step0/pty/{n}.tim'):
    p=l.split(None,2); t=float(p[0])
    if t<a or t>b: continue
    if p[1]=='OUT':
        print(f"{t:.3f} OUT {p[2].strip():>5} gap={'' if prev is None else f'{t-prev:.3f}'}"); prev=t
    else: print(f"{t:.3f} {p[1]} {p[2].strip() if len(p)>2 else ''}")
