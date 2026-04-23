#!/usr/bin/env bash
# QA readiness gate - validates implementation before review.
#
# Orchestrates the QA pipeline:
#   1. Validate evidence bundle exists
#   2. Run cargo test
#   3. Run cargo clippy
#   4. Run cargo fmt --check
#   5. Check for uncommitted changes
#   6. Generate summary report
#
# Usage:
#     ./scripts/qa_gate.sh 4
#     ./scripts/qa_gate.sh 4 --dry-run
#     ./scripts/qa_gate.sh 4 --focus-path crates/fusiongraph-core

set -euo pipefail

# Source cargo if not in PATH
if ! command -v cargo >/dev/null 2>&1 && [[ -n "${HOME:-}" ]] && [[ -f "$HOME/.cargo/env" ]]; then
    source "$HOME/.cargo/env"
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
REPO="Intelligence-Builder/FusionGraph"

ISSUE=""
DRY_RUN=0
FOCUS_PATHS=()
PASSED=0
FAILED=0
WARNINGS=0

usage() {
    cat <<'EOF'
Usage: ./scripts/qa_gate.sh <issue-number> [options]

Arguments:
    <issue-number>       GitHub issue number

Options:
    --dry-run            Show what would be done without executing
    --focus-path <path>  Focus on specific crate/path (repeatable)
    --skip-tests         Skip cargo test (for large test suites)
    -h, --help           Show this help

Examples:
    ./scripts/qa_gate.sh 4
    ./scripts/qa_gate.sh 4 --focus-path crates/fusiongraph-core
EOF
}

log() { printf "\033[1;34m[qa-gate]\033[0m %s\n" "$1"; }
warn() { printf "\033[1;33m[warn]\033[0m %s\n" "$1"; ((WARNINGS++)) || true; }
error() { printf "\033[1;31m[FAIL]\033[0m %s\n" "$1"; ((FAILED++)) || true; }
success() { printf "\033[1;32m[PASS]\033[0m %s\n" "$1"; ((PASSED++)) || true; }

SKIP_TESTS=0

# Parse arguments
if [[ $# -lt 1 ]]; then
    usage
    exit 2
fi

ISSUE="$1"
shift

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --focus-path) FOCUS_PATHS+=("$2"); shift 2 ;;
        --skip-tests) SKIP_TESTS=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

if ! [[ "$ISSUE" =~ ^[0-9]+$ ]]; then
    echo "Issue number must be numeric: $ISSUE" >&2
    exit 2
fi

cd "$REPO_ROOT"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "QA Readiness Gate - Issue #$ISSUE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Build cargo args for focused paths
CARGO_ARGS=""
if [[ ${#FOCUS_PATHS[@]} -gt 0 ]]; then
    for fp in "${FOCUS_PATHS[@]}"; do
        CRATE=$(basename "$fp")
        CARGO_ARGS="$CARGO_ARGS -p $CRATE"
    done
    log "Focusing on:$CARGO_ARGS"
fi

# Step 1: Check evidence bundle
log "Step 1: Checking evidence bundle..."
EVIDENCE_DIR=".code-foundry/issues/$ISSUE"
if [[ -d "$EVIDENCE_DIR" ]]; then
    if [[ -f "$EVIDENCE_DIR/discovery.md" ]]; then
        # Check if discovery questions are answered
        if grep -q "_(answer here)_" "$EVIDENCE_DIR/discovery.md"; then
            warn "Discovery questions not fully answered in $EVIDENCE_DIR/discovery.md"
        else
            success "Evidence bundle complete"
        fi
    else
        error "Missing discovery.md in $EVIDENCE_DIR"
    fi
else
    error "Evidence bundle not found: $EVIDENCE_DIR"
    echo "  Run: ./scripts/devwork.sh $ISSUE"
fi

# Step 2: cargo test
if [[ "$SKIP_TESTS" -eq 0 ]]; then
    log "Step 2: Running cargo test..."
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[dry-run] cargo test$CARGO_ARGS"
        ((PASSED++)) || true
    else
        if cargo test $CARGO_ARGS 2>&1; then
            success "All tests passed"
        else
            error "Tests failed"
        fi
    fi
else
    log "Step 2: Skipping tests (--skip-tests)"
fi

# Step 3: cargo clippy
log "Step 3: Running cargo clippy..."
if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] cargo clippy$CARGO_ARGS -- -D warnings"
    ((PASSED++)) || true
else
    if cargo clippy $CARGO_ARGS -- -D warnings 2>&1; then
        success "No clippy warnings"
    else
        error "Clippy warnings found"
    fi
fi

# Step 4: cargo fmt check
log "Step 4: Checking formatting..."
if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] cargo fmt -- --check"
    ((PASSED++)) || true
else
    if cargo fmt -- --check 2>&1; then
        success "Code is formatted"
    else
        error "Code needs formatting (run: cargo fmt)"
    fi
fi

# Step 5: Check for uncommitted changes
log "Step 5: Checking git status..."
UNCOMMITTED=$(git status --porcelain)
if [[ -z "$UNCOMMITTED" ]]; then
    success "Working tree clean"
else
    warn "Uncommitted changes detected:"
    echo "$UNCOMMITTED" | head -10
fi

# Step 6: Check branch is ahead of main
log "Step 6: Checking branch status..."
CURRENT_BRANCH=$(git branch --show-current)
COMMITS_AHEAD=$(git rev-list --count main.."$CURRENT_BRANCH" 2>/dev/null || echo "0")
if [[ "$COMMITS_AHEAD" -gt 0 ]]; then
    success "Branch has $COMMITS_AHEAD commit(s) ahead of main"
else
    warn "Branch has no commits ahead of main"
fi

# Summary
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "QA Gate Summary"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  Passed:   $PASSED"
echo "  Failed:   $FAILED"
echo "  Warnings: $WARNINGS"
echo ""

if [[ "$FAILED" -gt 0 ]]; then
    echo "❌ QA gate FAILED - fix issues before review"
    exit 1
elif [[ "$WARNINGS" -gt 0 ]]; then
    echo "⚠️  QA gate PASSED with warnings"
    echo ""
    echo "Next steps:"
    echo "  1. Address warnings if needed"
    echo "  2. git push"
    echo "  3. ./scripts/complete_issue.sh $ISSUE <changed-files>"
    exit 0
else
    echo "✅ QA gate PASSED"
    echo ""
    echo "Next steps:"
    echo "  1. git push"
    echo "  2. ./scripts/complete_issue.sh $ISSUE <changed-files>"
    exit 0
fi
