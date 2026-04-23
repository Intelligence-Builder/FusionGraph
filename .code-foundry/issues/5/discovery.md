# Discovery: Issue #5

## Issue
**Title:** Define GraphTableProvider trait signature
**State:** OPEN

## Discovery Questions

### Implementation Approach
The current provider surface is implemented in `crates/fusiongraph-datafusion/src/provider.rs` as a concrete `GraphTableProvider` wrapper around ontology and schema metadata, with the crate re-exported through `src/lib.rs`. The code establishes the DataFusion-facing provider shell and metadata accessors that later execution-plan work depends on, even though parts of the API reference remain aspirational beyond what the current concrete type exposes.

### Test Strategy
Unit coverage in `provider.rs` checks provider construction, returned schema, and label accessors. Workspace QA on 2026-04-22 additionally reran the full Rust test suite together with strict `clippy` and formatting checks so the provider layer is validated inside the same dependency graph as the execution operators.

### Risk Assessment
This slice is still heavier on API shape than runtime behavior. The issue requirements mention lazy materialization and traversal-plan creation methods that are described in `docs/FusionGraph_API_Reference.md`, but the runtime crate currently stops at the provider shell and a stubbed `scan` path. QA should treat that gap as part of acceptance review rather than assuming the documentation surface is fully implemented.

## Files Changed
- `crates/fusiongraph-datafusion/src/provider.rs`
- `crates/fusiongraph-datafusion/src/lib.rs`
- `docs/FusionGraph_API_Reference.md`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
