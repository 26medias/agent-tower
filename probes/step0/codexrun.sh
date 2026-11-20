#!/bin/bash
# codexrun.sh <codex args...> : clean env, temp CODEX_HOME, probe hooks via -c, luna + low effort, --no-daemon.
# Env: NOTRUST=1 skips the projects trust -c. UPDATE=1 passes no check_for_update flag (default: also none). NOHOOKS=1 skips hooks.
HOOKARGS=()
if [ -z "$NOHOOKS" ]; then while IFS= read -r l; do HOOKARGS+=(-c "$l"); done < /tmp/agent-tower-step0/codex-hook-args.txt; fi
TR=()
[ -z "$NOTRUST" ] && TR=(-c "projects={\"$PWD\"={trust_level=\"trusted\"}}")
exec codex --no-daemon "${HOOKARGS[@]}" "${TR[@]}" -m gpt-6-luna -c model_reasoning_effort=low "$@"
