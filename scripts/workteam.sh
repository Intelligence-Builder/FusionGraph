#!/usr/bin/env bash
# WorkTeam wrapper for FusionGraph.
#
# Dispatches workflow commands through a unified interface.
#
# Modes:
#   devwork  -> scripts/devwork.sh <issue> [--focus-path PATH]*
#   qa       -> scripts/qa_gate.sh <issue> [--focus-path PATH]*
#   test     -> cargo test [--package CRATE]
#   coverage -> cargo llvm-cov (if installed)
#
# Usage:
#   ./scripts/workteam.sh --mode devwork --issue 4
#   ./scripts/workteam.sh --mode qa --issue 4 --focus-path crates/fusiongraph-core
#   ./scripts/workteam.sh --mode test --focus-path crates/fusiongraph-core

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
MODE=""
ISSUE=""
FOCUS_PATHS=()
DRY_RUN=0

usage() {
    cat <<'EOF'
Usage:
  ./scripts/workteam.sh --mode <devwork|qa|test|coverage> [options]

Required:
  --mode <mode>            One of: devwork, qa, test, coverage

Options:
  --issue <number>         Issue number (required for devwork, qa)
  --focus-path <path>      Focus path (repeatable). Crate path for Rust.
  --dry-run                Print the dispatched command instead of running it
  -h, --help               Show this help and exit 0

Modes:
  devwork   Start work on an issue (branch, evidence bundle, preflight)
  qa        Run QA readiness gate (tests, clippy, fmt)
  test      Run cargo test with optional focus
  coverage  Run cargo llvm-cov for code coverage

Examples:
  ./scripts/workteam.sh --mode devwork --issue 4
  ./scripts/workteam.sh --mode qa --issue 4 --focus-path crates/fusiongraph-core
  ./scripts/workteam.sh --mode test --focus-path crates/fusiongraph-core
  ./scripts/workteam.sh --mode coverage

Notes:
  - The wrapper refuses to dispatch any command containing --no-verify.
  - Exit codes: 0 on success, 2 on usage error, non-zero from dispatched command.
EOF
}

run_cmd() {
    for arg in "$@"; do
        if [[ "$arg" == "--no-verify" ]]; then
            echo "ERROR: --no-verify is forbidden in wrapper-mediated flows" >&2
            return 1
        fi
    done

    if [[ "$DRY_RUN" -eq 1 ]]; then
        printf "[dry-run]"
        for arg in "$@"; do printf " %s" "$arg"; done
        printf "\n"
        return 0
    fi

    printf "[run]"
    for arg in "$@"; do printf " %s" "$arg"; done
    printf "\n"
    "$@"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode) MODE="${2:-}"; shift 2 ;;
        --issue) ISSUE="${2:-}"; shift 2 ;;
        --focus-path) FOCUS_PATHS+=("${2:-}"); shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

if [[ -z "$MODE" ]]; then
    echo "error: --mode is required" >&2
    usage
    exit 2
fi

cd "$REPO_ROOT"

case "$MODE" in
    devwork)
        if [[ -z "$ISSUE" ]]; then
            echo "error: --issue is required for --mode devwork" >&2
            exit 2
        fi
        cmd=("$REPO_ROOT/scripts/devwork.sh" "$ISSUE")
        for fp in "${FOCUS_PATHS[@]:-}"; do
            [[ -z "$fp" ]] && continue
            cmd+=("--focus-path" "$fp")
        done
        if [[ "$DRY_RUN" -eq 1 ]]; then
            cmd+=("--dry-run")
        fi
        run_cmd "${cmd[@]}"
        ;;

    qa)
        if [[ -z "$ISSUE" ]]; then
            echo "error: --issue is required for --mode qa" >&2
            exit 2
        fi
        cmd=("$REPO_ROOT/scripts/qa_gate.sh" "$ISSUE")
        for fp in "${FOCUS_PATHS[@]:-}"; do
            [[ -z "$fp" ]] && continue
            cmd+=("--focus-path" "$fp")
        done
        if [[ "$DRY_RUN" -eq 1 ]]; then
            cmd+=("--dry-run")
        fi
        run_cmd "${cmd[@]}"
        ;;

    test)
        cmd=(cargo test)
        for fp in "${FOCUS_PATHS[@]:-}"; do
            [[ -z "$fp" ]] && continue
            CRATE=$(basename "$fp")
            cmd+=("-p" "$CRATE")
        done
        run_cmd "${cmd[@]}"
        ;;

    coverage)
        if ! command -v cargo-llvm-cov &> /dev/null; then
            echo "cargo-llvm-cov not installed. Install with:"
            echo "  cargo install cargo-llvm-cov"
            exit 1
        fi
        cmd=(cargo llvm-cov)
        for fp in "${FOCUS_PATHS[@]:-}"; do
            [[ -z "$fp" ]] && continue
            CRATE=$(basename "$fp")
            cmd+=("-p" "$CRATE")
        done
        run_cmd "${cmd[@]}"
        ;;

    *)
        echo "error: unsupported mode '$MODE'" >&2
        usage
        exit 2
        ;;
esac
