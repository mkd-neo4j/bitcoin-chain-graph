#!/usr/bin/env bash
set -uo pipefail

# Shared PreToolUse guard for Bash commands.
# Blocks dangerous commands before Claude executes them.
# Exit 2 = block the command. Exit 0 = allow.

CMD=$(jq -r '.tool_input.command')

# Block rm -rf targeting root or home (not subdirectories like /tmp/safe)
if echo "$CMD" | grep -qE 'rm\s+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*\s+(.*\s)?(/|~)(\s|$)'; then
  echo "Blocked: destructive rm targeting / or ~" >&2
  exit 2
fi

# Block git push --force (but not --force-with-lease which is safe)
if echo "$CMD" | grep -qE 'git\s+push\s+.*--force(\s|$)'; then
  echo "Blocked: git push --force (use --force-with-lease instead)" >&2
  exit 2
fi

# Block git push -f shorthand
if echo "$CMD" | grep -qE 'git\s+push\s+(.*\s)?-f(\s|$)'; then
  echo "Blocked: git push -f (use --force-with-lease instead)" >&2
  exit 2
fi

# Block DROP DATABASE/TABLE
if echo "$CMD" | grep -qEi 'DROP\s+(DATABASE|TABLE)'; then
  echo "Blocked: DROP DATABASE/TABLE" >&2
  exit 2
fi

# Block direct commits on main/master when agent teams is active (feature-docs/ exists)
if [ -d "feature-docs" ] && echo "$CMD" | grep -qE 'git\s+commit'; then
  BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
  if [ "$BRANCH" = "main" ] || [ "$BRANCH" = "master" ]; then
    echo "Blocked: direct commit on $BRANCH — create a feature branch first" >&2
    exit 2
  fi
fi

exit 0
