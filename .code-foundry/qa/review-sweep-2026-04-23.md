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

Initial QA disposition: all reviewed issue cards were moved from `Review` back to `In Progress` because each corresponding PR branch had a failing QA gate.

## Devwork Follow-Up

The blocker branches were updated on 2026-04-23. Each issue branch now has complete evidence, a clean working tree, and a passing focused QA gate.

| Issue | Branch head | Command | Result |
| --- | --- | --- | --- |
| #2 | `feature/issue-2-blueprint-reorder` @ `7692239` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 2 --focus-path crates/fusiongraph-core` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #4 | `fix/issue-4-proptest` @ `728d464` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 4 --focus-path crates/fusiongraph-ontology` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #5 | `fix/issue-5-api-alignment` @ `762778f` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 5 --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #6 | `fix/issue-6-shard-partitioning` @ `7d3e1c9` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 6 --focus-path crates/fusiongraph-core` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #7 | `fix/issue-7-traversal-execute` @ `df34b2c` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 7 --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #8 | `fix/issue-8-integration-test` @ `9b372ca` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 8 --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #10 | `fix/issue-10-ffi-safety` @ `956fefa` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 10 --focus-path crates/fusiongraph-ffi` | Passed: `Passed 6 / Failed 0 / Warnings 0` |
| #12 | `feature/issue-12-simd-abstraction` @ `0eef456` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 12 --focus-path crates/fusiongraph-core` | Passed: `Passed 6 / Failed 0 / Warnings 0` |

Current board disposition: all eight issue cards have been moved back to `Review`. Issue comments now include the exact next-pass QA command, expected result, evidence file, and issue-specific checks to preserve.

## Review QA Sweep Rerun

Rerun scope on 2026-04-23: all project cards with `Status = Review` at the start of the rerun:

- #2 / PR #20 / `feature/issue-2-blueprint-reorder`
- #4 / PR #23 / `fix/issue-4-proptest`
- #5 / PR #22 / `fix/issue-5-api-alignment`
- #6 / PR #24 / `fix/issue-6-shard-partitioning`
- #7 / PR #25 / `fix/issue-7-traversal-execute`
- #8 / PR #26 / `fix/issue-8-integration-test`
- #10 / PR #27 / `fix/issue-10-ffi-safety`
- #11 / PR #28 / `feat/11-circuit-breaker`
- #12 / PR #21 / `feature/issue-12-simd-abstraction`

Changed-package QA was used for branches touching multiple Rust packages.

| Issue | Branch head | Command | Result | Board update |
| --- | --- | --- | --- | --- |
| #2 | `feature/issue-2-blueprint-reorder` @ `a2f2bb2` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 2 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #4 | `fix/issue-4-proptest` @ `728d464` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 4 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-ontology` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #5 | `fix/issue-5-api-alignment` @ `762778f` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 5 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #6 | `fix/issue-6-shard-partitioning` @ `7d3e1c9` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 6 --focus-path crates/fusiongraph-core` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #7 | `fix/issue-7-traversal-execute` @ `df34b2c` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 7 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #8 | `fix/issue-8-integration-test` @ `9b372ca` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 8 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #10 | `fix/issue-10-ffi-safety` @ `956fefa` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 10 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion --focus-path crates/fusiongraph-ffi` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |
| #11 | `feat/11-circuit-breaker` @ `de926fa` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 11 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Failed: `Passed 4 / Failed 1 / Warnings 1` | `In Progress` |
| #12 | `feature/issue-12-simd-abstraction` @ `0eef456` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 12 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Done` |

#11 blockers before the next QA pass:

- Complete `.code-foundry/issues/11/discovery.md`; it still contains `_(answer here)_` placeholders.
- Run `cargo fmt` and commit the formatting changes shown by QA in `crates/fusiongraph-core/src/error.rs` and `crates/fusiongraph-datafusion/src/exec/csr_builder.rs`.
- Rerun the exact #11 command above with a clean working tree; expected result is `Passed 6 / Failed 0 / Warnings 0`.

## Issue #11 Devwork Follow-Up

#11 was updated on 2026-04-23 and is ready for the next QA sweep.

| Issue | Branch head | Command | Result | Board update |
| --- | --- | --- | --- | --- |
| #11 | `feat/11-circuit-breaker` @ `5432f38` | `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 11 --focus-path crates/fusiongraph-core --focus-path crates/fusiongraph-datafusion` | Passed: `Passed 6 / Failed 0 / Warnings 0` | `Review` |

Follow-up changes:

- Completed `.code-foundry/issues/11/discovery.md` with implementation, test, risk, changed-file, and QA-result evidence.
- Committed the formatting changes previously reported by QA.
- Added the QA-proven core strict-Clippy cleanup so touching `fusiongraph-core` no longer re-exposes repo-wide Clippy blockers during the #11 gate.
