#!/usr/bin/env bash
set -uo pipefail

# TeammateIdle hook: scans feature-docs/ for pending work and redirects idle agents.
# Exit 2 = send directive to keep teammate working. Exit 0 = let teammate go idle.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
FEATURE_DIR="${PROJECT_DIR}/feature-docs"

# If no feature-docs directory, nothing to do
if [ ! -d "${FEATURE_DIR}" ]; then
  exit 0
fi

# Priority 1: Features with failing tests need a builder
for doc in "${FEATURE_DIR}"/testing/*.md; do
  [ -f "${doc}" ] || continue
  TITLE=$(grep -m1 '^title:' "${doc}" | sed 's/^title:[[:space:]]*//')
  echo "Pending work found: ${TITLE} needs implementation (status: testing)" >&2
  echo "Pick up feature-docs/testing/$(basename "${doc}") — read the feature doc and failing tests, then implement until all tests pass." >&2
  exit 2
done

# Priority 2: Features ready for test-writing
for doc in "${FEATURE_DIR}"/ready/*.md; do
  [ -f "${doc}" ] || continue
  TITLE=$(grep -m1 '^title:' "${doc}" | sed 's/^title:[[:space:]]*//')
  echo "Pending work found: ${TITLE} needs tests (status: ready)" >&2
  echo "Pick up feature-docs/ready/$(basename "${doc}") — read the feature doc and write failing tests for all acceptance criteria." >&2
  exit 2
done

# Priority 3: Features waiting for review
for doc in "${FEATURE_DIR}"/review/*.md; do
  [ -f "${doc}" ] || continue
  TITLE=$(grep -m1 '^title:' "${doc}" | sed 's/^title:[[:space:]]*//')
  echo "Pending work found: ${TITLE} needs review (status: review)" >&2
  echo "Review feature-docs/review/$(basename "${doc}") — check code quality, conventions, and test coverage." >&2
  exit 2
done

# No pending work
exit 0
