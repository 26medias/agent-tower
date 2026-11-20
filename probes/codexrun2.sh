#!/bin/bash
# codexrun2.sh <codex args...>  -- like codexrun.sh, but the hook list file comes from $HOOKFILE.
HOOKARGS=()
while IFS= read -r l; do HOOKARGS+=(-c "$l"); done < "${HOOKFILE:-/tmp/agent-tower-probes/codex-hook-args.txt}"
exec codex "${HOOKARGS[@]}" -c "projects={\"$PWD\"={trust_level=\"trusted\"}}" -m gpt-6-luna -c model_reasoning_effort=low "$@"
