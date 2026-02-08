#!/usr/bin/env bash
# Bitcoin Chain Graph - Verification Script
# Called by Claude Code Stop hook. Exit 2 = block Claude from finishing.
set -uo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

FAILURES=0

# ─── Early exit: skip verification if no Rust source files were modified ──
if git diff --quiet HEAD -- 'src/*.rs' 'tests/*.rs' Cargo.toml Cargo.lock 2>/dev/null \
   && git diff --cached --quiet HEAD -- 'src/*.rs' 'tests/*.rs' Cargo.toml Cargo.lock 2>/dev/null \
   && [ -z "$(git ls-files --others --exclude-standard src/ tests/ 2>/dev/null | grep '\.rs$')" ]; then
    exit 0
fi

step() {
    echo ""
    echo -e "${YELLOW}==> $1${NC}"
}

pass() {
    echo -e "${GREEN}  PASS: $1${NC}"
}

fail() {
    echo -e "${RED}  FAIL: $1${NC}"
    FAILURES=$((FAILURES + 1))
}

# ─── Formatting ─────────────────────────────────────────────────
step "Checking formatting (cargo fmt --check)"
if cargo fmt -- --check > /dev/null 2>&1; then
    pass "All files formatted correctly"
else
    fail "Files need formatting. Run: cargo fmt"
fi

# ─── Clippy ─────────────────────────────────────────────────────
step "Running clippy (cargo clippy -- -D warnings)"
if cargo clippy -- -D warnings 2>&1 | tail -3; then
    pass "No clippy warnings"
else
    fail "Clippy warnings found. Run: cargo clippy -- -D warnings"
fi

# ─── Type Check ─────────────────────────────────────────────────
step "Type checking (cargo check)"
if cargo check 2>&1 | tail -3; then
    pass "Type check passed"
else
    fail "Type check failed. Run: cargo check"
fi

# ─── Tests ──────────────────────────────────────────────────────
step "Running tests (cargo test)"
if cargo test 2>&1 | tail -5; then
    pass "All tests passed"
else
    fail "Tests failed. Run: cargo test"
fi

# ─── Banned Patterns ────────────────────────────────────────────
step "Checking for banned patterns in src/ (production code only)"

# Helper: scan only production code in each file (stops at #[cfg(test)])
# This avoids false positives from inline test modules in src/ files.
# Args: $1=pattern, $2=exclude_pattern (optional)
scan_prod() {
    local pattern="$1"
    local exclude="${2:-NOMATCH_XYZZY_PLACEHOLDER}"
    while IFS= read -r -d '' f; do
        awk "/#\\[cfg\\(test\\)\\]/{exit} /$pattern/ && !/\\/\\/\\// && !/\\/\\/!/ && !/$exclude/ {print FILENAME\":\"NR\": \"\$0}" "$f" 2>/dev/null
    done < <(find src/ -name '*.rs' -print0)
}

# Check for unwrap() in production code (excludes test modules, doc comments, unwrap_or variants)
UNWRAP_HITS=$(scan_prod '\\.unwrap()' 'unwrap_or')
UNWRAP_COUNT=$(echo "$UNWRAP_HITS" | grep -c '\.unwrap()' 2>/dev/null || true)
if [ "$UNWRAP_COUNT" -gt 0 ] && [ -n "$(echo "$UNWRAP_HITS" | tr -d '[:space:]')" ]; then
    fail "Found bare .unwrap() calls in production code (use .context()?, .expect(), or handle the error)"
    echo "$UNWRAP_HITS" | head -5
else
    pass "No bare .unwrap() in production code"
fi

# Check for println! in production library code (excludes main.rs, doc comments, test modules)
PRINTLN_HITS=$(
    while IFS= read -r -d '' f; do
        awk '/#\[cfg\(test\)\]/{exit} /println!/ && !/\/\/\// && !/\/\/!/ {print FILENAME":"NR": "$0}' "$f" 2>/dev/null
    done < <(find src/ -name '*.rs' ! -name 'main.rs' -print0)
)
PRINTLN_COUNT=$(echo "$PRINTLN_HITS" | grep -c 'println!' 2>/dev/null || true)
if [ "$PRINTLN_COUNT" -gt 0 ] && [ -n "$(echo "$PRINTLN_HITS" | tr -d '[:space:]')" ]; then
    fail "Found println! calls in production library code (use tracing macros instead)"
    echo "$PRINTLN_HITS" | head -5
else
    pass "No println! in library code"
fi

# Check for TODO without issue reference (production code only)
TODO_HITS=$(scan_prod 'TODO' 'TODO(#\\|TODO(http\\|TODO(issue')
TODO_COUNT=$(echo "$TODO_HITS" | grep -c 'TODO' 2>/dev/null || true)
if [ "$TODO_COUNT" -gt 0 ] && [ -n "$(echo "$TODO_HITS" | tr -d '[:space:]')" ]; then
    fail "Found TODO comments without issue references (use TODO(#123) or TODO(https://...))"
    echo "$TODO_HITS" | head -5
else
    pass "All TODOs have issue references"
fi

# Check for unsafe code (allows Mmap::map — the one legitimate use for memory-mapped I/O)
UNSAFE_HITS=$(scan_prod 'unsafe ' 'Mmap::map')
UNSAFE_COUNT=$(echo "$UNSAFE_HITS" | grep -c 'unsafe ' 2>/dev/null || true)
if [ "$UNSAFE_COUNT" -gt 0 ] && [ -n "$(echo "$UNSAFE_HITS" | tr -d '[:space:]')" ]; then
    fail "Found unsafe code in src/ (only Mmap::map is allowed)"
    echo "$UNSAFE_HITS" | head -5
else
    pass "No unauthorized unsafe code"
fi

# ─── Summary ────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ "$FAILURES" -gt 0 ]; then
    echo -e "${RED}VERIFICATION FAILED: $FAILURES issue(s) found${NC}"
    echo "Fix the issues above before finishing."
    exit 2
else
    echo -e "${GREEN}VERIFICATION PASSED: All checks clean${NC}"
    exit 0
fi
