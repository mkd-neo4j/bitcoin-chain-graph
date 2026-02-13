#!/usr/bin/env bash
set -uo pipefail

# TaskCompleted hook: runs lifecycle compliance check + full verify pipeline.
# Exit 2 = block task completion. Exit 0 = allow.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
FEATURE_DIR="${PROJECT_DIR}/feature-docs"
VERIFY_SCRIPT="${PROJECT_DIR}/scripts/verify.sh"

# --- Lifecycle compliance check ---
# Verify that every feature doc's status: frontmatter matches its directory.
# This catches agents that skip the "move feature doc" step.
if [ -d "${FEATURE_DIR}" ]; then
  LIFECYCLE_FAIL=0
  for status_dir in ready testing building review completed; do
    dir="${FEATURE_DIR}/${status_dir}"
    [ -d "${dir}" ] || continue
    for doc in "${dir}"/*.md; do
      [ -f "${doc}" ] || continue
      # Extract status from YAML frontmatter (first occurrence between --- delimiters)
      DOC_STATUS=$(awk '/^---$/{if(++c==2)exit} c==1 && /^status:/{sub(/^status:[[:space:]]*/, ""); print}' "${doc}")
      if [ -z "${DOC_STATUS}" ]; then
        continue  # No status field — skip (STATUS.md, CLAUDE.md, etc.)
      fi
      # Normalize: completed directory may use status: done or status: completed
      if [ "${status_dir}" = "completed" ]; then
        if [ "${DOC_STATUS}" = "done" ] || [ "${DOC_STATUS}" = "completed" ]; then
          continue
        fi
      fi
      if [ "${DOC_STATUS}" != "${status_dir}" ]; then
        BASENAME=$(basename "${doc}")
        echo "LIFECYCLE ERROR: ${BASENAME} is in ${status_dir}/ but has status: ${DOC_STATUS}" >&2
        echo "  Fix: update the status field to '${status_dir}' or move the file to ${DOC_STATUS}/" >&2
        LIFECYCLE_FAIL=1
      fi
    done
  done
  if [ "${LIFECYCLE_FAIL}" -eq 1 ]; then
    echo "" >&2
    echo "TaskCompleted BLOCKED — feature doc lifecycle violation detected." >&2
    echo "Move the feature doc to the correct directory and update its status: field." >&2
    exit 2
  fi
fi

# --- Lifecycle-aware verify skip ---
# During testing stage, tests are supposed to fail — skip verify.
# Lifecycle compliance already ran above; the builder will verify when it completes.
. "${PROJECT_DIR}/scripts/lifecycle-stage.sh"
if [ "$LIFECYCLE_STAGE" = "testing" ]; then
  echo "Stage: testing — skipping verify (tests expected to fail)" >&2
  exit 0
fi

# --- Full verify pipeline ---
if [ ! -f "${VERIFY_SCRIPT}" ]; then
  echo "Warning: scripts/verify.sh not found — skipping verification" >&2
  exit 0
fi

OUTPUT=$(bash "${VERIFY_SCRIPT}" 2>&1) || {
  echo "TaskCompleted BLOCKED — verify.sh failed:" >&2
  echo "${OUTPUT}" | tail -30 >&2
  exit 2
}

exit 0
