# FusionGraph - Claude AI Instructions

## Project Overview

FusionGraph is a Zero-ETL graph execution layer that integrates Apache DataFusion with a CSR-based graph kernel. It projects Iceberg/Parquet data lakes into navigable in-memory graphs without data movement.

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

- arrow 54.x
- datafusion 45.x
- crossbeam-epoch for memory reclamation
- dashmap for lock-free concurrent maps

## Key Design Decisions

1. **64MB micro-shards**: CSR partitioned for cache efficiency
2. **Lock-free delta layer**: DashMap for real-time edge mutations
3. **Arrow FFI**: Zero-copy data transfer via C Data Interface
4. **Epoch-based reclamation**: Safe memory management for concurrent access
