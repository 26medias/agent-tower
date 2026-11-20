#!/bin/sh
# Usage: render.sh <name> [cols] [rows]  -- replays the raw PTY output in a private tmux and prints the screen
n=$1; c=${2:-160}; r=${3:-45}
tmux -L atrender kill-session -t r 2>/dev/null
tmux -L atrender new-session -d -s r -x $c -y $r "cat /tmp/agent-tower-probes/pty/$n.out; sleep 30"
sleep 1.5
tmux -L atrender capture-pane -p -t r | grep -v '^\s*$'
tmux -L atrender kill-session -t r
