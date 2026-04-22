# Discovery: Issue #9

## Issue
**Title:** Basic BFS traversal (scalar)
**State:** OPEN

## Discovery Questions

### Implementation Approach
Scalar BFS lives in `crates/fusiongraph-core/src/traversal/bfs.rs`, backed by neighbor iteration from `src/csr.rs`. The implementation tracks visited nodes, per-node depths, level-grouped output, and max-depth cutoffs, and it uses the same base-plus-delta adjacency iteration that the core graph structure exposes to higher layers.

### Test Strategy
The BFS test suite exercises traversal from a root node, max-depth truncation, zero-depth traversal, nonexistent starts, level construction, and multi-start behavior. Workspace validation on 2026-04-22 also reran `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --check`.

### Risk Assessment
The traversal logic covers the scalar baseline the issue asks for, but the current repository does not include a benchmark harness that proves the SIMD-comparison baseline called out in the acceptance criteria. QA should evaluate algorithm correctness separately from later performance work.

## Files Changed
- `crates/fusiongraph-core/src/traversal/bfs.rs`
- `crates/fusiongraph-core/src/traversal.rs`
- `crates/fusiongraph-core/src/csr.rs`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
