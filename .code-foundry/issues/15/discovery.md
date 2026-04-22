# Discovery: Issue #15

## Issue
**Title:** These `#[tokio::test] async fn` examples use `?` but the functions are declared to return `()`, so the snippets as written won’t compile. Update the signatures to return `Result<(), _>` (or use explicit `unwrap/expect`) so the examples are copy/pasteable.
**State:** OPEN

## Discovery Questions

### Implementation Approach
This issue is the runnable-docs fix for the async examples in `docs/FusionGraph_Testing_Strategy.md`. The review feedback targeted `#[tokio::test]` examples that used `?` while still returning `()`, so the implementation work is to make those snippets copy-pasteable by returning `Result<(), _>` or otherwise removing the invalid error propagation.

### Test Strategy
Validation is documentation review plus repo health checks: confirm the affected snippets in `docs/FusionGraph_Testing_Strategy.md` compile conceptually, then keep the workspace green with `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --check`. Those workspace checks all passed on 2026-04-22.

### Risk Assessment
The code risk is low because this issue is docs-only, but the usability risk is real: broken example signatures undercut the credibility of the testing guide and slow down adoption. Future doc changes should continue to treat example code as executable reference material rather than prose.

## Files Changed
- `docs/FusionGraph_Testing_Strategy.md`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
