# Discovery: Issue #15

## Issue
**Title:** These `#[tokio::test] async fn` examples use `?` but the functions are declared to return `()`, so the snippets as written won’t compile. Update the signatures to return `Result<(), _>` (or use explicit `unwrap/expect`) so the examples are copy/pasteable.
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
