# Discovery: Issue #8

## Issue
**Title:** CSRBuilderExec physical operator
**State:** OPEN

## Discovery Questions

### Implementation Approach
`CSRBuilderExec` is implemented in `crates/fusiongraph-datafusion/src/exec/csr_builder.rs` as the DataFusion physical operator that consumes `RecordBatch` input, validates source/target columns, builds CSR state, and enforces memory limits and partition constraints. This slice is the bridge between Arrow/DataFusion input streams and the core CSR builder.

### Test Strategy
The datafusion test suite covers empty input handling, successful CSR builds, custom column names, memory-limit failures, invalid column types, missing source columns, partitioning invariants, multi-batch input, and CSRBuilderExec metadata reporting. Focused workteam QA on 2026-04-23 reran tests, clippy, and formatting for `fusiongraph-core`, `fusiongraph-datafusion`, and `fusiongraph-ontology`.

### Risk Assessment
The builder path is substantially more complete than the traversal operator, but the issue's performance target and spill-to-disk expectations are still broader than the current unit evidence. QA should treat throughput and large-input behavior as distinct from the correctness checks already present in the repository.

## Files Changed
- `.code-foundry/issues/8/discovery.md`
- `crates/fusiongraph-core/src/csr.rs`
- `crates/fusiongraph-core/src/csr/builder.rs`
- `crates/fusiongraph-core/src/csr/shard.rs`
- `crates/fusiongraph-core/src/delta.rs`
- `crates/fusiongraph-core/src/error.rs`
- `crates/fusiongraph-core/src/lib.rs`
- `crates/fusiongraph-core/src/traversal/bfs.rs`
- `crates/fusiongraph-core/src/types.rs`
- `crates/fusiongraph-datafusion/src/exec/csr_builder.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 8 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion --focus-path crates/fusiongraph-ontology`: passed on 2026-04-23.
- Unit tests passed: `45` core, `29` datafusion, and `21` ontology tests.
- Clippy passed with no warnings; formatting passed.
