#!/bin/sh
# run-claude.sh <runname> <cwd> <claude args...>  : clean env, probe hooks, haiku, in PTY driver
n=$1; cwd=$2; shift 2
cd "$cwd" && exec /tmp/agent-tower-step0/cleanenv.sh AT_RUN=$n python3 /tmp/agent-tower-step0/ptydrive.py $n 120 40 -- claude --settings /tmp/agent-tower-step0/claude-settings.json --model haiku "$@"
