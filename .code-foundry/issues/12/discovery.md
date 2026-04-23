# Discovery: Issue #12

## Issue
**Title:** Design SIMD abstraction layer for cross-platform support
**State:** OPEN

## Discovery Questions

### Implementation Approach
The SIMD abstraction is implemented in `crates/fusiongraph-core/src/traversal/simd.rs` and exposed through `crates/fusiongraph-core/src/traversal.rs`. The module defines a `SimdBackend` trait, scalar fallback, x86_64 AVX2/AVX-512 backends behind conditional compilation, an aarch64 Neon backend, and a scalar fallback for all other targets. Runtime selection uses `is_x86_feature_detected!` on x86_64, compiles directly to Neon on aarch64, and falls back to scalar elsewhere.

### Test Strategy
Unit tests cover scalar unvisited filtering, batch visited-bit updates, runtime backend selection, selected backend name consistency, and parity between the selected backend and scalar implementation. The QA gate runs the issue-focused core package tests, strict `clippy`, formatting, evidence, and clean-tree checks.

### Risk Assessment
The backend trait and platform selection strategy are in place, but the AVX2, AVX-512, and Neon implementations currently delegate to the scalar backend until architecture-specific intrinsics are implemented. That is intentional for this design issue because it proves cross-platform compilation and identical semantics before adding unsafe or target-feature-specific optimized code.

## Files Changed
- `crates/fusiongraph-core/src/traversal.rs`
- `crates/fusiongraph-core/src/traversal/simd.rs`
- `crates/fusiongraph-core/src/lib.rs`

## Test Results
- `CARGO_TARGET_DIR=/tmp/fusiongraph-qa/target ./scripts/workteam.sh --mode qa --issue 12 --focus-path crates/fusiongraph-core`: passed on 2026-04-23 (`Passed 5 / Failed 0 / Warnings 1`; warning was uncommitted changes before final commit)
