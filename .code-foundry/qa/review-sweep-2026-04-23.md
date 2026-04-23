# Review QA Sweep - 2026-04-23

Project: <https://github.com/orgs/Intelligence-Builder/projects/13/views/1>

Scope: all project cards with `Status = Review` at sweep start:

- #2 / PR #20 / `feature/issue-2-blueprint-reorder`
- #4 / PR #23 / `fix/issue-4-proptest`
- #5 / PR #22 / `fix/issue-5-api-alignment`
- #6 / PR #24 / `fix/issue-6-shard-partitioning`
- #7 / PR #25 / `fix/issue-7-traversal-execute`
- #8 / PR #26 / `fix/issue-8-integration-test`
- #10 / PR #27 / `fix/issue-10-ffi-safety`
- #12 / PR #21 / `feature/issue-12-simd-abstraction`

## Summary

No reviewed PR branch passed the QA gate. Tests passed for #2, #5, #6, #7, #8, #10, and #12. Issue #4 failed before runtime tests because the added proptest generator does not match the current `NodeDefinition` schema.

Common blockers:

- Review PR branches do not include the shared core clippy fixes from `codex/qa-blockers-evidence`, so `cargo clippy -- -D warnings` fails in `fusiongraph-core`.
- Several branches contain rustfmt drift.
- Evidence bundles are missing or still contain unanswered discovery placeholders.

## Results

| Issue | Command | Result | Required before next QA pass |
| --- | --- | --- | --- |
| #2 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 2 --focus-path crates/fusiongraph-datafusion` | Failed: `Passed 4 / Failed 1 / Warnings 1` | Complete `.code-foundry/issues/2/discovery.md`; make the PR branch match the issue scope or retitle/rescope it, because the current diff changes `fusiongraph-datafusion` files rather than the blueprint dependency-order docs; include the shared core clippy cleanup or rebase on a branch that has it. |
| #4 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 4 --focus-path crates/fusiongraph-ontology` | Failed: `Passed 3 / Failed 3 / Warnings 0` | Add `.code-foundry/issues/4/discovery.md`; fix `crates/fusiongraph-ontology/src/parser.rs` proptest generation to construct `NodeDefinition { id_column: IdColumn::Single(...), id_transform: ... }`; run `cargo fmt`; rerun the QA gate. |
| #5 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 5 --focus-path crates/fusiongraph-datafusion` | Failed: `Passed 3 / Failed 3 / Warnings 0` | Add `.code-foundry/issues/5/discovery.md`; run `cargo fmt`; include the shared core clippy cleanup or rebase on a branch that has it; rerun the QA gate. |
| #6 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 6 --focus-path crates/fusiongraph-core` | Failed: `Passed 3 / Failed 3 / Warnings 0` | Add `.code-foundry/issues/6/discovery.md`; run `cargo fmt`; resolve `fusiongraph-core` clippy warnings including shard-size cast/precision lints; rerun the QA gate. |
| #7 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 7 --focus-path crates/fusiongraph-datafusion` | Failed: `Passed 3 / Failed 2 / Warnings 1` | Complete `.code-foundry/issues/7/discovery.md`; run `cargo fmt`; include the shared core clippy cleanup or rebase on a branch that has it; rerun the QA gate. |
| #8 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 8 --focus-path crates/fusiongraph-datafusion` | Failed: `Passed 3 / Failed 2 / Warnings 1` | Complete `.code-foundry/issues/8/discovery.md`; run `cargo fmt`; include the shared core clippy cleanup or rebase on a branch that has it; rerun the QA gate. |
| #10 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 10 --focus-path crates/fusiongraph-ffi` | Failed: `Passed 3 / Failed 3 / Warnings 0` | Add `.code-foundry/issues/10/discovery.md`; run `cargo fmt`; include the shared core clippy cleanup or rebase on a branch that has it; rerun the QA gate. |
| #12 | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 12 --focus-path crates/fusiongraph-core` | Failed: `Passed 3 / Failed 2 / Warnings 1` | Complete `.code-foundry/issues/12/discovery.md`; run `cargo fmt`; fix SIMD-specific clippy warnings in `crates/fusiongraph-core/src/traversal/simd.rs`; include the shared core clippy cleanup or rebase on a branch that has it; rerun the QA gate. |

## Board Updates

All reviewed issue cards should be moved from `Review` back to `In Progress` because each corresponding PR branch has a failing QA gate.
