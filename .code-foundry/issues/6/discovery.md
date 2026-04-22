# Discovery: Issue #6

## Issue
**Title:** CSRShard struct and micro-sharding layout
**State:** OPEN

## Discovery Questions

### Implementation Approach
The micro-sharding foundation is implemented in `crates/fusiongraph-core/src/csr/shard.rs`, `src/csr.rs`, and `src/csr/builder.rs`. `CsrShard` owns the row-pointer and column-index storage, while `CsrGraph` provides global-to-shard and shard-to-global mapping plus neighbor iteration over the base and delta layers. The builder currently emits a single shard and preserves the indexing/memory model needed for later multi-shard expansion.

### Test Strategy
The core test suite covers shard basics, containment, out-degree lookup, neighbor ranges, checksum stability, and graph-level indexing roundtrips. Post-fix QA on 2026-04-22 also reran the full workspace tests, strict `clippy`, and formatting checks after tightening the CSR code to satisfy the repo's pedantic lint policy.

### Risk Assessment
The data structures and indexing logic are in place, but true multi-shard partitioning and memory-overhead benchmarking remain follow-on work. The current implementation is production-shaped for API review, not the final 64MB partitioning strategy implied by the blueprint's large-graph performance goals.

## Files Changed
- `crates/fusiongraph-core/src/csr.rs`
- `crates/fusiongraph-core/src/csr/builder.rs`
- `crates/fusiongraph-core/src/csr/shard.rs`
- `crates/fusiongraph-core/src/lib.rs`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
