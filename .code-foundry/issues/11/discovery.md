# Discovery: Issue #11

## Issue
**Title:** Implement error handling from Error Taxonomy spec
**State:** OPEN

## Discovery Questions

### Implementation Approach
Issue #11 extends the error-handling implementation with a circuit breaker for external dependency protection. The new `crates/fusiongraph-core/src/circuit_breaker.rs` module defines `CircuitState`, `CircuitBreakerConfig`, and `CircuitBreaker`, using atomics for thread-safe state transitions across closed, open, and half-open states. The implementation returns `GraphError::CircuitOpen` when calls should fail fast, and `crates/fusiongraph-core/src/lib.rs` exports the module for downstream integration.

The branch also carries `CSRBuilderExec` integration-test coverage in `crates/fusiongraph-datafusion/src/exec/csr_builder.rs` from the dependent QA work. That coverage remains in the changed-package QA command so this branch is validated against both touched crates.

### Test Strategy
Circuit breaker unit tests cover default closed state, opening after the configured failure threshold, success reset behavior, manual reset behavior, half-open success recovery, half-open failure reopening, and the retryable `GraphError::CircuitOpen` code path. The QA command also runs the touched `fusiongraph-datafusion` tests because this branch includes `CSRBuilderExec` integration-test changes.

### Risk Assessment
The circuit breaker currently tracks time using wall-clock milliseconds and atomics, which is adequate for this implementation slice but should be revisited if production code needs injectable clocks or deterministic timeout testing. External dependency call sites are not wired through this breaker yet; that integration remains follow-on work after the core primitive passes QA.

## Files Changed
- `.code-foundry/issues/11/discovery.md`
- `crates/fusiongraph-core/src/circuit_breaker.rs`
- `crates/fusiongraph-core/src/lib.rs`
- `crates/fusiongraph-datafusion/src/exec/csr_builder.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 11 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion`: passed on 2026-04-23.
- Unit tests passed: `52` core and `29` datafusion tests.
- Clippy passed with no warnings; formatting passed.
