# Discovery: Issue #16

## Issue
**Title:** This markdown table has an extra leading `|` (it starts with `|| Layer ...`), which renders as an empty first column in many markdown parsers. Change the table rows to start with a single `|` so the columns align correctly.
**State:** OPEN

## Discovery Questions

### Implementation Approach
This issue also came from PR #13 review feedback on `docs/FusionGraph_Testing_Strategy.md`. The implementation is a formatting correction in the markdown table near the top of the document so the header and rows render with the intended columns instead of an empty leading column caused by `|| Layer ...`.

### Test Strategy
Validation is primarily visual and review-based: inspect the markdown table in `docs/FusionGraph_Testing_Strategy.md` and confirm it renders as a standard table with aligned columns. Workspace checks on 2026-04-22 also passed for `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --check`.

### Risk Assessment
This is a low-risk docs cleanup, but it matters because malformed tables make the testing guidance harder to scan and easier to misread. The main residual risk is future markdown edits regressing the formatting without a dedicated docs render check.

## Files Changed
- `docs/FusionGraph_Testing_Strategy.md`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
