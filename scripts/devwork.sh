#!/usr/bin/env bash
# Developer workflow wrapper - canonical entry point for starting work on an issue.
#
# Orchestrates the developer pre-work setup:
#   1. Fetch issue context from GitHub
#   2. Initialize evidence bundle directory
#   3. Create/checkout feature branch
#   4. Run preflight checks (cargo check, clippy)
#   5. Print summary with next-step commands
#
# Usage:
#     ./scripts/devwork.sh 4
#     ./scripts/devwork.sh 4 --dry-run
#     ./scripts/devwork.sh 4 --skip-preflight

set -euo pipefail

# Source cargo if not in PATH
if ! command -v cargo >/dev/null 2>&1; then
    CARGO_ENV="${HOME:-}/.cargo/env"
    [[ -n "${HOME:-}" && -f "$CARGO_ENV" ]] && source "$CARGO_ENV"
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
REPO="Intelligence-Builder/FusionGraph"

ISSUE=""
DRY_RUN=0
SKIP_PREFLIGHT=0
FOCUS_PATHS=()

usage() {
    cat <<'EOF'
Usage: ./scripts/devwork.sh <issue-number> [options]

Arguments:
    <issue-number>       GitHub issue number to work on

Options:
    --dry-run            Show what would be done without executing
    --skip-preflight     Skip cargo check and clippy preflight
    --focus-path <path>  Focus on specific crate/path (repeatable)
    -h, --help           Show this help

Examples:
    ./scripts/devwork.sh 4
    ./scripts/devwork.sh 4 --focus-path crates/fusiongraph-core
    ./scripts/devwork.sh 4 --dry-run
EOF
}

log() { printf "\033[1;34m[devwork]\033[0m %s\n" "$1"; }
warn() { printf "\033[1;33m[warn]\033[0m %s\n" "$1"; }
error() { printf "\033[1;31m[error]\033[0m %s\n" "$1" >&2; }
success() { printf "\033[1;32m[ok]\033[0m %s\n" "$1"; }

run_cmd() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        printf "[dry-run] %s\n" "$*"
        return 0
    fi
    "$@"
}

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
        --skip-preflight) SKIP_PREFLIGHT=1; shift ;;
        --focus-path) FOCUS_PATHS+=("$2"); shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) error "Unknown argument: $1"; usage; exit 2 ;;
    esac
done

if ! [[ "$ISSUE" =~ ^[0-9]+$ ]]; then
    error "Issue number must be numeric: $ISSUE"
    exit 2
fi

cd "$REPO_ROOT"

# Step 1: Fetch issue context
log "Fetching issue #$ISSUE from GitHub..."
ISSUE_JSON=$(gh issue view "$ISSUE" --repo "$REPO" --json title,body,labels,state 2>/dev/null || echo "{}")
ISSUE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.title // "Unknown"')
ISSUE_STATE=$(echo "$ISSUE_JSON" | jq -r '.state // "UNKNOWN"')

if [[ "$ISSUE_TITLE" == "Unknown" ]]; then
    warn "Could not fetch issue #$ISSUE - continuing anyway"
else
    success "Issue: $ISSUE_TITLE"
    echo "  State: $ISSUE_STATE"
fi

# Step 2: Initialize evidence bundle
EVIDENCE_DIR=".code-foundry/issues/$ISSUE"
log "Initializing evidence bundle..."

if [[ "$DRY_RUN" -eq 0 ]]; then
    mkdir -p "$EVIDENCE_DIR"

    # Create discovery.md if it doesn't exist
    if [[ ! -f "$EVIDENCE_DIR/discovery.md" ]]; then
        cat > "$EVIDENCE_DIR/discovery.md" <<DISCOVERY
# Discovery: Issue #$ISSUE

## Issue
**Title:** $ISSUE_TITLE
**State:** $ISSUE_STATE

## Discovery Questions

### Implementation Approach
_(answer here)_

### Test Strategy
_(answer here)_

### Risk Assessment
_(answer here)_

## Files Changed
<!-- Updated after implementation -->

## Test Results
<!-- cargo test output -->
DISCOVERY
        success "Created $EVIDENCE_DIR/discovery.md"
    else
        success "Evidence bundle exists"
    fi
else
    echo "[dry-run] Would create $EVIDENCE_DIR/discovery.md"
fi

# Step 3: Create/checkout feature branch
BRANCH_NAME="feature/issue-$ISSUE"
CURRENT_BRANCH=$(git branch --show-current)

log "Setting up feature branch..."
if [[ "$CURRENT_BRANCH" == "$BRANCH_NAME" ]]; then
    success "Already on branch $BRANCH_NAME"
elif git show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
    run_cmd git checkout "$BRANCH_NAME"
    success "Checked out existing branch $BRANCH_NAME"
else
    run_cmd git checkout -b "$BRANCH_NAME"
    success "Created new branch $BRANCH_NAME"
fi

# Step 4: Preflight checks
if [[ "$SKIP_PREFLIGHT" -eq 0 ]]; then
    log "Running preflight checks..."

    CARGO_ARGS=""
    if [[ ${#FOCUS_PATHS[@]} -gt 0 ]]; then
        for fp in "${FOCUS_PATHS[@]}"; do
            # Extract crate name from path like "crates/fusiongraph-core"
            CRATE=$(basename "$fp")
            CARGO_ARGS="$CARGO_ARGS -p $CRATE"
        done
    fi

    echo "  cargo check$CARGO_ARGS"
    if [[ "$DRY_RUN" -eq 0 ]]; then
        if cargo check $CARGO_ARGS 2>&1 | tail -5; then
            success "cargo check passed"
        else
            warn "cargo check had issues"
        fi
    fi

    echo "  cargo clippy$CARGO_ARGS"
    if [[ "$DRY_RUN" -eq 0 ]]; then
        if cargo clippy $CARGO_ARGS -- -D warnings 2>&1 | tail -5; then
            success "cargo clippy passed"
        else
            warn "cargo clippy had warnings"
        fi
    fi
else
    log "Skipping preflight checks (--skip-preflight)"
fi

# Step 5: Summary
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Ready to work on issue #$ISSUE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Branch:   $BRANCH_NAME"
echo "Evidence: $EVIDENCE_DIR/discovery.md"
echo ""
echo "Next steps:"
echo "  1. Answer discovery questions in $EVIDENCE_DIR/discovery.md"
echo "  2. Implement the feature/fix"
echo "  3. Run: cargo test"
echo "  4. Run: ./scripts/qa_gate.sh $ISSUE"
echo "  5. Run: ./scripts/complete_issue.sh $ISSUE <changed-files>"
echo ""
