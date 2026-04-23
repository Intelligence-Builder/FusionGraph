# Discovery: Issue #10

## Issue
**Title:** Arrow C Data Interface (zero-copy FFI)
**State:** OPEN

## Discovery Questions

### Implementation Approach
The FFI bridge is implemented in `crates/fusiongraph-ffi/src/lib.rs` using Arrow's `FFI_ArrowArray` and `FFI_ArrowSchema` helpers for import/export. The crate converts struct-backed Arrow data into `RecordBatch` values and exports batches back through the Arrow C Data Interface, while the C-compatible result and stats structs define the outward ABI shape.

### Test Strategy
The FFI crate includes round-trip coverage for export and import, including edge batches, empty batches, large batches, repeated round-trips, and default stats. Focused workteam QA on 2026-04-23 reran tests, clippy, and formatting for `fusiongraph-core`, `fusiongraph-datafusion`, `fusiongraph-ffi`, and `fusiongraph-ontology`.

### Risk Assessment
The current tests prove import/export behavior inside Rust, but the issue's stronger requirements around Miri and external zero-copy benchmarking are not represented as dedicated automation in this repository. The ABI surface is present, but low-level cross-language validation remains an area where future evidence would strengthen acceptance.

## Files Changed
- `.code-foundry/issues/10/discovery.md`
- `crates/fusiongraph-ffi/src/lib.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 10 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion --focus-path crates/fusiongraph-ffi --focus-path crates/fusiongraph-ontology`: passed on 2026-04-23.
- Unit tests passed: `45` core, `29` datafusion, `7` ffi, and `21` ontology tests.
- Clippy passed with no warnings; formatting passed.
