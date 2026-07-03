# FusionGraph - Claude AI Instructions

## Project Overview

FusionGraph is a Zero-ETL graph execution layer that integrates Apache DataFusion with a CSR-based graph kernel. It projects Iceberg/Parquet data lakes into navigable in-memory graphs without data movement.

**Positioning:** the open-source, embeddable, DataFusion-native graph engine
(vs. PuppyGraph = closed server product, DuckPGQ = DuckDB-only, Kùzu = archived 2025).

## Scope Guardrails (read before implementing anything)

The committed scope lives in `docs/ROADMAP.md`. All other documents in `docs/`
are **vision documents** — do not implement features from them unless they
appear in the roadmap milestones.

- **Benchmark-first:** the core claim (CSR traversal beats join-based multi-hop
  SQL on the same engine) must stay measured. Do not merge features ahead of
  the benchmark that justifies them. Benches: `cargo bench -p fusiongraph-core`
  and `cargo bench -p fusiongraph-datafusion`.
- **Cross-lint before pushing:** dev machines are Apple Silicon; the AVX2 code
  path only gets linted on x86_64. Run
  `cargo clippy --target x86_64-apple-darwin --workspace --all-targets -- -D warnings`
  locally (CI runs the equivalent on ubuntu and *will* catch it otherwise).
- **CI** (`.github/workflows/ci.yml`) runs fmt + clippy `-D warnings` + tests
  on ubuntu (x86_64, validates AVX2 kernel) and macOS (aarch64, validates
  NEON), plus a no-default-features check and bench compilation.
- **Deferred (kill list):** AVX-512 intrinsics, datafusion-tokomak/e-graphs,
  Snowflake Native App/Horizon, ReflexArc agentic layer, Substrait,
  hot/warm/cold tiering. Reject PRs implementing these without a roadmap change.
- **SIMD policy:** `SimdBackend` trait with runtime dispatch exists; all
  backends currently delegate to scalar. Vectorize NEON first (dev machines are
  Apple Silicon), AVX2 second, and only after profiling (roadmap M4).

## Crate Structure

| Crate | Purpose |
|-------|---------|
| fusiongraph-core | CSR storage, delta layer, BFS/DFS traversal |
| fusiongraph-ontology | TOML/JSON schema parser and validation |
| fusiongraph-datafusion | DataFusion TableProvider and ExecutionPlan operators |
| fusiongraph-ffi | Arrow C Data Interface bindings |

## Development Workflow

### WorkTeam Wrapper (Canonical Entry Point)

```bash
# Start work on an issue (creates branch, evidence bundle, runs preflight)
./scripts/workteam.sh --mode devwork --issue <N>

# Focus on specific crate
./scripts/workteam.sh --mode devwork --issue <N> --focus-path crates/fusiongraph-core

# Dry-run to preview commands
./scripts/workteam.sh --mode devwork --issue <N> --dry-run
```

### After Implementation

```bash
# Run QA readiness gate
./scripts/workteam.sh --mode qa --issue <N>

# Or run tests directly
./scripts/workteam.sh --mode test --focus-path crates/fusiongraph-core

# Commit with conventional format
git commit -m "feat(crate): description

Closes #N"

# Post commit reference to issue
./scripts/complete_issue.sh <N> <changed-files>
```

### Direct Script Access

```bash
# Start work on issue (branch + evidence bundle + preflight)
./scripts/devwork.sh <N>

# Run QA gate (tests + clippy + fmt + git checks)
./scripts/qa_gate.sh <N>

# Post commit reference
./scripts/complete_issue.sh <N> <files>
```

### PR Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] Conventional commit message
- [ ] PR references related issue

## Code Standards

### Error Codes

Use structured error codes: `FG-<CRATE>-E<NNN>`
- `FG-CSR-E001` through `FG-CSR-E00N` for core CSR errors
- `FG-ONT-E001` through `FG-ONT-E00N` for ontology errors
- `FG-FFI-E001` through `FG-FFI-E00N` for FFI errors

### Testing

- Unit tests in each module with `#[cfg(test)]`
- Integration tests in `tests/` directories
- Property-based tests for data structures (proptest)

### Dependencies

- arrow 55.x
- datafusion 47.x
- iceberg + iceberg-datafusion 0.5.x (feature `iceberg`, default-on in fusiongraph-datafusion)
- crossbeam-epoch for memory reclamation
- dashmap for lock-free concurrent maps

## Key Design Decisions

1. **64MB micro-shards**: CSR partitioned for cache efficiency
2. **Lock-free delta layer**: DashMap for real-time edge mutations
3. **Arrow FFI**: Zero-copy data transfer via C Data Interface
4. **Epoch-based reclamation**: Safe memory management for concurrent access
5. **Portable SIMD via trait dispatch**: scalar reference implementation is
   canonical; platform intrinsics are an optimization detail, not architecture
6. **Benchmark-gated development**: comparative benches (CSR vs. SQL joins)
   are the acceptance test for the project's core claim
