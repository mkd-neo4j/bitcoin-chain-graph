#!/usr/bin/env bash
set -uo pipefail

# TaskCompleted hook: runs full verify pipeline before a task can be marked done.
# Exit 2 = block task completion. Exit 0 = allow.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
VERIFY_SCRIPT="${PROJECT_DIR}/scripts/verify.sh"

if [ ! -f "${VERIFY_SCRIPT}" ]; then
  echo "Warning: scripts/verify.sh not found — skipping verification" >&2
  exit 0
fi

# Run the full verify pipeline
OUTPUT=$(bash "${VERIFY_SCRIPT}" 2>&1) || {
  echo "TaskCompleted BLOCKED — verify.sh failed:" >&2
  echo "${OUTPUT}" | tail -30 >&2
  exit 2
}

exit 0
