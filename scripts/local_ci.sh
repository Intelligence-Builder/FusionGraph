#!/usr/bin/env bash
# Local CI - Run the same checks that CI would run.
#
# Executes quality gates in sequence:
#   1. cargo fmt --check (formatting)
#   2. cargo clippy (linting)
#   3. cargo test (tests)
#   4. cargo doc (documentation)
#   5. cargo audit (security - if installed)
#
# Usage:
#     ./scripts/local_ci.sh
#     ./scripts/local_ci.sh --quick       # Skip tests and docs
#     ./scripts/local_ci.sh -p fusiongraph-core  # Focus on one crate

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
QUICK=0
CARGO_ARGS=""
FAILED=0
PASSED=0

usage() {
    cat <<'EOF'
Usage: ./scripts/local_ci.sh [options]

Options:
    --quick              Skip tests and doc generation
    -p, --package <pkg>  Focus on specific package/crate
    --no-audit           Skip cargo audit even if installed
    -h, --help           Show this help

Examples:
    ./scripts/local_ci.sh
    ./scripts/local_ci.sh --quick
    ./scripts/local_ci.sh -p fusiongraph-core
EOF
}

log() { printf "\n\033[1;36m━━━ %s ━━━\033[0m\n" "$1"; }
pass() { printf "\033[1;32m✓ %s\033[0m\n" "$1"; ((PASSED++)) || true; }
fail() { printf "\033[1;31m✗ %s\033[0m\n" "$1"; ((FAILED++)) || true; }

NO_AUDIT=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick) QUICK=1; shift ;;
        -p|--package) CARGO_ARGS="$CARGO_ARGS -p $2"; shift 2 ;;
        --no-audit) NO_AUDIT=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

cd "$REPO_ROOT"

echo ""
echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                    FusionGraph Local CI                      │"
echo "└─────────────────────────────────────────────────────────────┘"

# Step 1: Format check
log "Step 1/5: cargo fmt --check"
if cargo fmt -- --check; then
    pass "Formatting OK"
else
    fail "Formatting issues found (run: cargo fmt)"
fi

# Step 2: Clippy
log "Step 2/5: cargo clippy"
if cargo clippy $CARGO_ARGS -- -D warnings 2>&1; then
    pass "Clippy OK"
else
    fail "Clippy warnings found"
fi

# Step 3: Tests
if [[ "$QUICK" -eq 0 ]]; then
    log "Step 3/5: cargo test"
    if cargo test $CARGO_ARGS 2>&1; then
        pass "Tests passed"
    else
        fail "Tests failed"
    fi
else
    log "Step 3/5: cargo test (SKIPPED --quick)"
fi

# Step 4: Documentation
if [[ "$QUICK" -eq 0 ]]; then
    log "Step 4/5: cargo doc"
    if RUSTDOCFLAGS="-D warnings" cargo doc $CARGO_ARGS --no-deps 2>&1; then
        pass "Documentation OK"
    else
        fail "Documentation warnings found"
    fi
else
    log "Step 4/5: cargo doc (SKIPPED --quick)"
fi

# Step 5: Security audit
if [[ "$NO_AUDIT" -eq 0 ]] && command -v cargo-audit &> /dev/null; then
    log "Step 5/5: cargo audit"
    if cargo audit 2>&1; then
        pass "No known vulnerabilities"
    else
        fail "Security vulnerabilities found"
    fi
elif [[ "$NO_AUDIT" -eq 1 ]]; then
    log "Step 5/5: cargo audit (SKIPPED --no-audit)"
else
    log "Step 5/5: cargo audit (SKIPPED - not installed)"
    echo "  Install with: cargo install cargo-audit"
fi

# Summary
echo ""
echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                         Summary                              │"
echo "└─────────────────────────────────────────────────────────────┘"
echo ""
echo "  Passed: $PASSED"
echo "  Failed: $FAILED"
echo ""

if [[ "$FAILED" -gt 0 ]]; then
    echo "❌ Local CI FAILED"
    exit 1
else
    echo "✅ Local CI PASSED"
    exit 0
fi
