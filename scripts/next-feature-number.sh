#!/usr/bin/env bash
set -euo pipefail

# Returns the next available 3-digit feature number prefix.
# Scans all lifecycle directories + ideation folders for existing NNN- prefixes.
# Usage: bash next-feature-number.sh [feature-docs-dir]
#   Prints the next number, e.g. "004"

FEATURE_DIR="${1:-feature-docs}"

MAX=0

# Scan lifecycle directories for NNN-*.md files
for dir in ready testing building review completed; do
  target="${FEATURE_DIR}/${dir}"
  [ -d "${target}" ] || continue
  for f in "${target}"/[0-9][0-9][0-9]-*.md; do
    [ -f "${f}" ] || continue
    NUM=$(basename "${f}")
    NUM="${NUM%%-*}"
    NUM=$((10#${NUM}))
    [ "${NUM}" -gt "${MAX}" ] && MAX="${NUM}"
  done
done

# Scan ideation directories for NNN-* folders
if [ -d "${FEATURE_DIR}/ideation" ]; then
  for d in "${FEATURE_DIR}"/ideation/[0-9][0-9][0-9]-*/; do
    [ -d "${d}" ] || continue
    NUM=$(basename "${d}")
    NUM="${NUM%%-*}"
    NUM=$((10#${NUM}))
    [ "${NUM}" -gt "${MAX}" ] && MAX="${NUM}"
  done
fi

NEXT=$((MAX + 1))
printf '%03d\n' "${NEXT}"
