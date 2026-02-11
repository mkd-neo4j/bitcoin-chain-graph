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

# Fast verify first (type check only) — catches most errors quickly.
# Falls back to full verify only if fast-verify script exists.
FAST_SCRIPT="$CLAUDE_PROJECT_DIR"/scripts/fast-verify.sh
if [ -f "${FAST_SCRIPT}" ]; then
  OUT=$(bash "${FAST_SCRIPT}" 2>&1)
  RC=$?
  if [ $RC -ne 0 ]; then
    echo "$OUT" | tail -20 >&2
    exit $RC
  fi
  # Fast check passed — skip full verify on Stop (TaskCompleted handles it)
  exit 0
fi

# No fast-verify available — run full verification
OUT=$("$CLAUDE_PROJECT_DIR"/scripts/verify.sh 2>&1)
RC=$?
echo "$OUT" | tail -20 >&2
exit $RC
