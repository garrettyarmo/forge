#!/usr/bin/env bash
set -euo pipefail

# Configuration
MAX_ITERATIONS=10
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
B='\033[1;34m'   # Blue
GR='\033[1;32m'  # Green
Y='\033[1;33m'   # Yellow
R='\033[0m'      # Reset

for i in $(seq 1 $MAX_ITERATIONS); do
  echo -e "${B}▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓${R}"
  echo -e " 🔄 ${GR}Iteration ${Y}$i${R} of ${Y}$MAX_ITERATIONS${R}"
  echo -e "${B}▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓${R}"

  # Run claude with the ralph prompt
  OUTPUT=$(DOCKER_DEFAULT_PLATFORM=linux/amd64 docker sandbox run claude --model opus --permission-mode acceptEdits -p "$(cat $SCRIPT_DIR/.agent/PROMPT.md)" 2>&1) || true
  echo "$OUTPUT"

  # Check for completion signal
  if echo "$OUTPUT" | grep -q "<promise>COMPLETE</promise>"; then
    echo ""
    echo -e "${GR}▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓${R}"
    echo -e " 🎉 ${GR}Ralph completed all tasks!${R}"
    echo -e " ✅ Finished at iteration ${GR}$i${R} of ${GR}$MAX_ITERATIONS${R}"
    echo -e "${GR}▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓${R}"
    exit 0
  fi
done
