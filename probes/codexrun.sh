#!/bin/bash
# codexrun.sh <codex args...>  -- runs codex with the probe hooks as -c overrides,
# the probe repo trusted for this run only, and a small model.
HOOKARGS=()
while IFS= read -r l; do HOOKARGS+=(-c "$l"); done < /tmp/agent-tower-probes/codex-hook-args.txt
exec codex "${HOOKARGS[@]}" -c "projects={\"$PWD\"={trust_level=\"trusted\"}}" -m gpt-6-luna -c model_reasoning_effort=low "$@"
