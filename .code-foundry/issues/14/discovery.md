# Discovery: Issue #14

## Issue
**Title:** In the `GraphError` example, the `impl` references variants that are not declared in the shown enum (e.g., `CsrCorruption`, `IcebergConnectionFailed`, `CredentialExpired`, `MemoryLimitExceeded`, `NodeNotFound`, `CircuitOpen`). This makes the example internally inconsistent. Either add the missing variants to the enum snippet or adjust the `code/severity/is_retryable` examples to match the declared variants.
**State:** OPEN

## Discovery Questions

### Implementation Approach
_(answer here)_

### Test Strategy
_(answer here)_

### Risk Assessment
_(answer here)_

## Files Changed
<!-- Updated after implementation -->

## Test Results
<!-- cargo test output -->
