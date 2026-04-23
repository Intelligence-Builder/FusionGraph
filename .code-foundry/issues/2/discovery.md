# Discovery: Issue #2

## Issue
**Title:** Reorder Blueprint tasks by dependency
**State:** OPEN

## Discovery Questions

### Implementation Approach
The blueprint now includes a dedicated `Task Dependencies & Recommended Phase Order` section near the top of `docs/FusionGraph_Technical Blueprint.md`. The section makes the hard ordering explicit: `CSRShard` precedes Arrow FFI and CSRBuilderExec, GraphTraversalExec depends on SIMD BFS and CSRBuilderExec, and independent concurrency/optimizer work is deferred into a later concurrent phase. Unrelated DataFusion implementation files that were previously present on this branch were restored to `origin/main` to keep the issue focused on the documentation change.

### Test Strategy
This is a documentation issue, so validation checks that the dependency table and phase ordering are present in the blueprint. The formal QA gate also runs a focused core package test/clippy/format pass because the repository QA script requires a cargo package focus; the branch includes the shared core lint cleanup needed for that gate to pass cleanly.

### Risk Assessment
The documentation ordering is low-risk and aligns with the issue's dependency table. The main residual risk is process-related: future docs-only issues should avoid carrying unrelated implementation commits, otherwise QA reviewers must disentangle scope before accepting the branch.

## Files Changed
- `docs/FusionGraph_Technical Blueprint.md`
- `crates/fusiongraph-core/src/lib.rs`
- `crates/fusiongraph-core/src/csr.rs`
- `crates/fusiongraph-core/src/csr/builder.rs`
- `crates/fusiongraph-core/src/csr/shard.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 2 --focus-path crates/fusiongraph-core`: passed on 2026-04-23 (`Passed 5 / Failed 0 / Warnings 1`; warning was uncommitted changes before final commit)
