#!/usr/bin/env bash
set -uo pipefail

# Read hook input from stdin
INPUT=$(cat)

# Skip if hook is already active (prevents recursive loops)
ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null)
[ "$ACTIVE" = "true" ] && exit 0

# Skip verification if no files have been modified or created.
# Checks both tracked modifications and new untracked files.
# After setup.sh, commit everything so the working tree is clean —
# then this check correctly skips verify during conversations.
if git diff --quiet HEAD -- . 2>/dev/null && \
   [ -z "$(git ls-files --others --exclude-standard 2>/dev/null)" ]; then
  exit 0
fi

# Run verification
OUT=$("$CLAUDE_PROJECT_DIR"/scripts/verify.sh 2>&1)
RC=$?
echo "$OUT" | tail -30 >&2
exit $RC
