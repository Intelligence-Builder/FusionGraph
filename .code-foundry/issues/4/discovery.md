# Discovery: Issue #4

## Issue
**Title:** Implement Ontology parser (TOML/JSON)
**State:** OPEN

## Discovery Questions

### Implementation Approach
The ontology slice lives in `crates/fusiongraph-ontology`: `src/parser.rs` handles TOML/JSON decoding into the shared `Ontology` model, while `src/schema.rs` and `src/validation.rs` enforce duplicate-label, dangling-edge, computed-property, and temporal-edge rules from the spec. The implementation is structured so the parser normalizes both input formats before validation runs against the same in-memory shape.

### Test Strategy
Coverage comes from the ontology unit suite in `crates/fusiongraph-ontology`, including valid TOML/JSON parsing, invalid TOML rejection, node and edge extraction, duplicate label failures, dangling edge failures, and validation-context preservation. Post-fix QA on 2026-04-22 also reran `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --check`.

### Risk Assessment
The parser and validator cover the repository's documented structure and invariants, but the acceptance criterion about validating against a live catalog schema is still represented as model-level validation rather than an external catalog integration test. Future schema changes in `docs/FusionGraph_Ontology_Schema.md` must stay aligned with the Rust model and validator rules.

## Files Changed
- `crates/fusiongraph-ontology/src/parser.rs`
- `crates/fusiongraph-ontology/src/schema.rs`
- `crates/fusiongraph-ontology/src/validation.rs`
- `crates/fusiongraph-ontology/src/error.rs`

## Test Results
- `cargo test --workspace`: passed on 2026-04-22 (`35` core, `11` datafusion, `2` ffi, `16` ontology unit tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on 2026-04-22
- `cargo fmt --check`: passed on 2026-04-22
