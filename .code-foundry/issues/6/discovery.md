# Discovery: Issue #6

## Issue
**Title:** CSRShard struct and micro-sharding layout
**State:** OPEN

## Discovery Questions

### Implementation Approach
The micro-sharding foundation is implemented in `crates/fusiongraph-core/src/csr/shard.rs`, `src/csr.rs`, and `src/csr/builder.rs`. `CsrShard` owns the row-pointer and column-index storage, while `CsrGraph` provides global-to-shard and shard-to-global mapping plus neighbor iteration over the base and delta layers. The builder partitions CSR arrays into contiguous node-range shards based on the configurable shard-size target, preserving edge counts and weighted-edge slices across shard boundaries.

### Test Strategy
The core test suite covers shard basics, containment, out-degree lookup, neighbor ranges, checksum stability, graph-level indexing roundtrips, forced multi-shard partitioning, cross-shard neighbor access, boundary coverage, edge-count preservation, invalid shard/node handling, and weighted graph partitioning. QA reruns the issue-focused core package tests, strict `clippy`, formatting, evidence, and clean-tree checks.

### Risk Assessment
The indexing and shard partitioning path is covered by unit tests, but the large-scale 100M+ edge and memory-overhead benchmark remains a follow-on performance validation item. The implementation keeps shard sizing configurable and deterministic so a later benchmark can validate the 64MB target under production-scale datasets.

## Files Changed
- `crates/fusiongraph-core/src/csr.rs`
- `crates/fusiongraph-core/src/csr/builder.rs`
- `crates/fusiongraph-core/src/csr/shard.rs`
- `crates/fusiongraph-core/src/lib.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 6 --focus-path crates/fusiongraph-core`: passed on 2026-04-23 (`Passed 5 / Failed 0 / Warnings 1`; warning was uncommitted changes before final commit)
