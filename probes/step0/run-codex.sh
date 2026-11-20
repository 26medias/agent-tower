#!/bin/sh
# run-codex.sh <runname> <cwd> <codexrun args...>
n=$1; cwd=$2; shift 2
cd "$cwd" && exec /tmp/agent-tower-step0/cleanenv.sh AT_RUN=$n CODEX_HOME=/tmp/agent-tower-step0/codex-home python3 /tmp/agent-tower-step0/ptydrive.py $n 120 40 -- /tmp/agent-tower-step0/codexrun.sh "$@"
