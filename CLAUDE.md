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

### Starting Work on an Issue

```bash
# Create feature branch
git checkout -b feature/issue-<N>-short-desc

# Run tests before changes
cargo test

# Run clippy
cargo clippy -- -D warnings
```

### After Implementation

```bash
# Test and lint
cargo test
cargo clippy -- -D warnings

# Commit with conventional format
git commit -m "feat(crate): description

Closes #N"

# Post commit reference to issue
./scripts/complete_issue.sh <N> <changed-files>
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
