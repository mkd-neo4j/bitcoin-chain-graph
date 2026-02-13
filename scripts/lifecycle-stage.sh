#!/usr/bin/env bash
# Detects the active lifecycle stage from feature-docs/.
# Source this script; it sets LIFECYCLE_STAGE to: testing | building | review | none
# If multiple features are in different stages, the most permissive stage wins
# (testing > building > review) because one test-writer's unresolved imports
# poison the entire type checker.

_FEATURE_DIR="${CLAUDE_PROJECT_DIR:-${PROJECT_DIR:-$(pwd)}}/feature-docs"
LIFECYCLE_STAGE="none"

if [ -d "${_FEATURE_DIR}" ]; then
  for _stage in testing building review; do
    _dir="${_FEATURE_DIR}/${_stage}"
    [ -d "${_dir}" ] || continue
    for _doc in "${_dir}"/*.md; do
      [ -f "${_doc}" ] || continue
      if awk '/^---$/{if(++c==2)exit} c==1 && /^status:/{found=1} END{exit !found}' "${_doc}" 2>/dev/null; then
        LIFECYCLE_STAGE="${_stage}"
        break 2
      fi
    done
  done
fi
