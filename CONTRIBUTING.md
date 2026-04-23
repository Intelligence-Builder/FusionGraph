# Contributing to FusionGraph

Thank you for your interest in contributing to FusionGraph! This project aims to integrate high-performance graph processing directly into Apache DataFusion, and we welcome contributions from the Arrow/DataFusion/Rust community.

---

## Code of Conduct

FusionGraph follows the [Apache Code of Conduct](https://www.apache.org/foundation/policies/conduct.html). By participating, you agree to uphold this code. Please report unacceptable behavior to [project maintainers].

---

## How to Contribute

### 1. Report Bugs

If you find a bug, please [create an issue](https://github.com/Intelligence-Builder/FusionGraph/issues/new) with:
- **Clear title** describing the problem
- **Steps to reproduce** the issue
- **Expected vs actual behavior**
- **Environment details** (OS, Rust version, DataFusion version)
- **Minimal code example** demonstrating the issue

### 2. Request Features

Feature requests are welcome! Please [open a discussion](https://github.com/Intelligence-Builder/FusionGraph/discussions) first to:
- Explain the use case
- Describe the proposed solution
- Discuss implementation approaches
- Get community feedback

Once there's consensus, create an issue to track the work.

### 3. Contribute Code

We follow the standard GitHub fork-and-pull-request workflow:

1. **Fork the repository**
2. **Create a feature branch** from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Make your changes** following our coding standards (see below)
4. **Write tests** for your changes
5. **Run the test suite** to ensure nothing breaks:
   ```bash
   cargo test
   cargo clippy -- -D warnings
   cargo fmt --check
   ```
6. **Commit your changes** with clear, descriptive commit messages:
   ```
   Add SIMD-optimized neighbor traversal

   - Implement AVX-512 vectorized CSR traversal
   - Add micro-benchmarks for hot path
   - Document SIMD requirements in README

   Closes #123
   ```
7. **Push to your fork** and **submit a pull request**

### 4. Improve Documentation

Documentation improvements are highly valued! This includes:
- Architecture guides
- API documentation
- Tutorial examples
- Benchmark reports
- README clarifications

---

## Development Setup

### Prerequisites

- **Rust 1.75+** (install via [rustup](https://rustup.rs/))
- **Apache DataFusion 35+** (included as dependency)
- **AVX-512 capable CPU** (for SIMD benchmarks, optional)

### Clone and Build

```bash
# Clone your fork
git clone https://github.com/Intelligence-Builder/FusionGraph.git
cd fusiongraph

# Build the project
cargo build --release

# Run tests
cargo test

# Run clippy (linter)
cargo clippy -- -D warnings

# Format code
cargo fmt
```

### Project Structure

```
fusiongraph/
├── src/
│   ├── kernel/          # CSR core, SIMD traversal
│   │   ├── csr.rs       # Compressed Sparse Row implementation
│   │   ├── simd.rs      # AVX-512 optimized kernels
│   │   └── reclaim.rs   # Epoch-based memory reclamation
│   ├── lsm/             # LSM-Graph dual-layer architecture
│   │   ├── base.rs      # Immutable CSR base layer
│   │   ├── delta.rs     # DashMap delta layer
│   │   └── fusion.rs    # Wait-free view fusion
│   ├── datafusion/      # DataFusion integration
│   │   ├── operators.rs # Physical operators (CSRBuilderExec, GraphTraversalExec)
│   │   ├── substrait.rs # Substrait plan deserialization
│   │   └── optimizer.rs # E-graph query optimization
│   ├── catalog/         # Storage layer integration
│   │   ├── iceberg.rs   # Apache Iceberg catalog integration
│   │   └── pruning.rs   # Manifest-level pruning
│   └── lib.rs
├── benches/             # Criterion benchmarks
├── docs/                # Architecture documentation
└── examples/            # Usage examples
```

---

## Coding Standards

### Rust Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` for consistent formatting
- Pass `cargo clippy -- -D warnings` with no warnings
- Write idiomatic Rust (leverage type system, avoid unsafe unless necessary)

### Performance-Critical Code

FusionGraph is a performance-focused project. For hot-path code (CSR traversal, SIMD kernels):

1. **Profile before optimizing** — use `cargo bench` and `perf`
2. **Document SIMD assumptions** — AVX-512 availability, alignment requirements
3. **Use `#[inline]` judiciously** — on small, frequently called functions
4. **Avoid allocations in hot loops** — pre-allocate or use stack buffers
5. **Write micro-benchmarks** — add Criterion benches for new hot-path code

### Safety and Correctness

- **Prefer safe Rust** — use `unsafe` only when necessary
- **Document all `unsafe` blocks** — explain invariants and why they're upheld
- **Add assertions** — liberal use of `debug_assert!` for invariants
- **Write comprehensive tests** — unit tests, integration tests, property tests

---

## Testing

### Test Categories

1. **Unit tests** — in `src/` files with `#[cfg(test)]`
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_csr_neighbor_lookup() {
           // ...
       }
   }
   ```

2. **Integration tests** — in `tests/` directory
   ```bash
   cargo test --test datafusion_integration
   ```

3. **Benchmarks** — in `benches/` directory
   ```bash
   cargo bench
   ```

4. **Doc tests** — examples in documentation comments
   ```rust
   /// Traverses the graph using BFS
   /// 
   /// # Example
   /// ```
   /// let traversal = bfs(&graph, start_node);
   /// ```
   ```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_csr_neighbor_lookup

# With output
cargo test -- --nocapture

# Benchmarks
cargo bench

# Doc tests only
cargo test --doc
```

---

## Pull Request Guidelines

### Before Submitting

- [ ] Code builds without warnings: `cargo build --release`
- [ ] All tests pass: `cargo test`
- [ ] Clippy passes: `cargo clippy -- -D warnings`
- [ ] Code is formatted: `cargo fmt`
- [ ] New code has tests
- [ ] Documentation is updated (if public API changed)
- [ ] CHANGELOG.md is updated (for user-facing changes)

### PR Description Template

```markdown
## Summary
[Brief description of the change]

## Motivation
[Why is this change needed? What problem does it solve?]

## Changes
- [List of changes made]
- [Another change]

## Testing
[How did you test this change?]

## Checklist
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Benchmarks added (if performance-critical)
- [ ] CHANGELOG.md updated
```

### Review Process

1. **Automated checks** run on all PRs (tests, clippy, formatting)
2. **Maintainer review** — at least one maintainer approval required
3. **Community feedback** — other contributors may comment
4. **Revisions** — address review comments with new commits
5. **Merge** — squash-merge to main after approval

---

## Architecture Contribution Areas

### High-Priority Areas

1. **DataFusion Physical Operators**
   - Implement `CSRBuilderExec` from Arrow RecordBatches
   - Implement `GraphTraversalExec` for BFS/DFS/PageRank
   - Integrate with DataFusion's query planner

2. **SIMD Optimization**
   - AVX-512 neighbor traversal kernels
   - Vectorized edge weight processing
   - SIMD-friendly memory layout

3. **LSM-Graph Compaction**
   - Background delta-to-base merge
   - Epoch-based reclamation logic
   - Write-optimized delta layer design

4. **Substrait Support**
   - Deserialize Substrait plans with graph operators
   - E-graph optimization for multi-hop fusion
   - Cost model for graph vs relational operators

### Medium-Priority Areas

5. **Hot/Warm/Cold Tiering**
   - Access pattern tracking
   - Automatic promotion/demotion
   - Memory-mapped warm tier

6. **Benchmarking**
   - Comparison vs Neo4j, TigerGraph, Memgraph
   - Reproducible benchmark suite
   - Performance regression testing

7. **Iceberg Integration**
   - Manifest-level pruning
   - Schema mapping for graph semantics
   - Metadata extraction

### Future Exploration

8. **Distributed Execution**
   - Multi-node CSR sharding
   - Network-aware query planning
   - Fault tolerance

9. **GPU Acceleration**
   - CUDA kernels for traversal
   - GPU-CSR memory transfer
   - Hybrid CPU-GPU execution

---

## Communication Channels

- **GitHub Issues** — bug reports, feature requests
- **GitHub Discussions** — questions, ideas, design discussions
- **Apache Arrow Mailing List** — DataFusion integration topics
- **Discord** — real-time chat (link TBD)

---

## Maintainers

Current maintainers:
- Robert Stanley ([@Intelligence-Builder](https://github.com/Intelligence-Builder)) — Project lead

We're actively looking for co-maintainers from the Apache Arrow/DataFusion community!

---

## Attribution

Contributors will be recognized in:
- **CONTRIBUTORS.md** — all code contributors
- **Release notes** — for significant contributions
- **Documentation** — for documentation improvements

Thank you for contributing to FusionGraph! 🚀
