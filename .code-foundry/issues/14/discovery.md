# Discovery: Issue #14

## Issue
**Title:** In the `GraphError` example, the `impl` references variants that are not declared in the shown enum (e.g., `CsrCorruption`, `IcebergConnectionFailed`, `CredentialExpired`, `MemoryLimitExceeded`, `NodeNotFound`, `CircuitOpen`). This makes the example internally inconsistent. Either add the missing variants to the enum snippet or adjust the `code/severity/is_retryable` examples to match the declared variants.
**State:** OPEN

## Discovery Questions

### Implementation Approach
This issue came from PR #13 review feedback on `docs/FusionGraph_Error_Taxonomy.md`. The fix is documentation consistency: the example `GraphError` enum and the example helper methods now need to reference the same declared variants so implementers are not reading an internally contradictory error model. The runtime crate's `crates/fusiongraph-core/src/error.rs` is the code-side reference point for that alignment.

### Test Strategy
Because this is a docs-focused issue, validation is primarily review-based: confirm the spec text and example code agree on the declared variants, and then keep the repo green with workspace `cargo test`, strict `clippy`, and `cargo fmt --check`. Those workspace checks all passed on 2026-04-22 after the QA blocker cleanup.

### Risk Assessment
The residual risk is documentation drift. The runtime error enum can stay correct while the long-form taxonomy doc regresses independently, so future reviews should continue to treat the spec and implementation as two separate artifacts that must stay synchronized.

## Files Changed
- `docs/FusionGraph_Error_Taxonomy.md`
- `crates/fusiongraph-core/src/error.rs`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
