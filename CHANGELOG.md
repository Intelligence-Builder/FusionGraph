# Changelog

All notable changes to FusionGraph are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/).

## [Unreleased]

### Added

- **Delta auto-compaction policy** (#40): `CompactionPolicy` +
  `CsrGraph::should_compact` in core; `GraphCatalog::compact_if_needed`
  atomically swaps the registry entry and replays mutations that raced the
  compaction, so writers lose no data
- **Direction-optimizing BFS in SQL** (#38): `GraphCatalog` memoizes a lazy
  transpose per graph (`register_with_reverse` / `reverse`);
  `GraphTraversalExec::with_reverse` enables DO-BFS on outgoing traversals
  and executes `TraversalDirection::Incoming` as outgoing-on-transpose;
  `graph_traverse` gains a direction argument:
  `graph_traverse('g', 3, 5, 'in')` answers "who can reach node 3?"
- **Weighted + temporal ontology projection** (#39, partial):
  `weight_column` flows through `CsrBuildConfig` into weighted CSR storage
  (NULLs default to 1.0); `register_ontology_graphs_as_of` filters edges by
  `valid_from`/`valid_to` at an instant (string/UUID ID transforms remain
  open)
- **`GraphTableProvider` implemented** (#37): `new(ontology)` exposes the
  canonical merged edge list `(source, target, label)`; `materialize(ctx)`
  builds the merged CSR + scannable batches from all edge definitions;
  `scan` serves the governed edge list (projection + limit pushdown);
  `create_traversal_plan` returns a real `GraphTraversalExec`

### Changed

- `GraphTableProvider::new` signature: `(ontology, schema)` → `(ontology)`
  (the schema is now the canonical edge list)
- `graph_traverse` accepts 3–5 arguments (direction may take position 4 or 5)

## [0.1.0] — pending first publish

Initial release of all four crates. Publish runbook: `docs/RELEASING.md`.

### Added — core kernel (`fusiongraph-core`)

- CSR graph storage with 64MB micro-sharding (`CsrGraph`, `CsrShard`,
  `CsrBuilder`)
- Lock-free delta layer for live edge insertions/tombstones (`DeltaLayer`)
- **Delta → base compaction** (`CsrGraph::compact`): LSM-style merge that
  restores the traversal fast path (377 µs dirty → 3.9 µs compacted)
- **CSR transpose** (`CsrGraph::transpose`): incoming-edge traversal as
  outgoing BFS on the reversed topology
- BFS kernel (`bfs`, `bfs_multi`, `bfs_bounded`): dense-bitset visited
  tracking, zero-copy `&[u32]` neighbor slices, allocation-free SIMD batch
  filtering — 3-hop BFS at 10M edges in 6.8 µs
- **Direction-optimizing BFS** (`bfs_direction_optimized`, Beamer α/β
  heuristic): 2.2–3.2x on skewed hub traversals; 100M-edge BFS in 140 ms
  (~714M edges/sec)
- SIMD backend abstraction with runtime dispatch and real NEON + AVX2
  kernels, equivalence-tested against the scalar reference on both
  architectures in CI. Scalar is the measured default on `aarch64`
  (the filter is gather-bound; NEON measured ~5% slower)
- Deterministic graph generators (`gen::uniform`, `gen::rmat` with
  Graph500 parameters)
- Structured error taxonomy (`FG-<SUBSYSTEM>-<SEVERITY><NNN>`) and
  circuit breaker

### Added — DataFusion integration (`fusiongraph-datafusion`)

- `CSRBuilderExec`: physical operator projecting `RecordBatch` streams into
  CSR graphs, with `GraphSink` for downstream graph handoff
- `GraphTraversalExec`: BFS as a native `ExecutionPlan` (~5 µs overhead
  over the raw kernel)
- **`graph_traverse` SQL table function** backed by `GraphCatalog`
  (projection + `LIMIT` pushdown; composes with joins/filters/aggregates)
- **Ontology-driven registration** (`register_ontology_graphs`):
  `fusiongraph.toml` edge definitions become named, SQL-queryable graphs
- **Apache Iceberg support** (default-on `iceberg` feature):
  `register_iceberg_table` and snapshot-pinned
  `register_iceberg_table_snapshot` via the official `iceberg-datafusion`
  provider (manifest-based file pruning included)
- Runnable examples: `graph_traverse` (Parquet blast-radius demo) and
  `iceberg_graph` (snapshot-pinned audit scenario)

### Added — schema (`fusiongraph-ontology`)

- TOML/JSON ontology parser, schema types, and validation
  (dangling-edge/duplicate-label detection)

### Added — FFI (`fusiongraph-ffi`)

- Arrow C Data Interface bindings for zero-copy batch exchange

### Added — project infrastructure

- Criterion benchmark suites: kernel microbenches, R-MAT tiers (incl.
  opt-in 100M-edge via `FG_BENCH_LARGE=1`), CSR-vs-SQL comparisons on
  MemTable/Parquet/Iceberg, recursive-CTE baseline (semantics-asserted
  against BFS)
- GitHub Actions CI: fmt + clippy `-D warnings` + tests on x86_64 Linux
  and aarch64 macOS, no-default-features check, bench compilation
- Roadmap (`docs/ROADMAP.md`), release runbook (`docs/RELEASING.md`),
  docs.rs embedding guide (crate-level docs)

### Measured (see README for methodology)

| Claim | Number |
|---|---|
| 3-hop k-hop, 10M edges, operator vs. idiomatic `WITH RECURSIVE` | ~7,500x |
| 3-hop k-hop, 10M edges, operator vs. hand-tuned chained joins | ~2,190x |
| Parquet → CSR projection, 10M edges | 204 ms |
| Iceberg → CSR projection, 10M edges | 174 ms |
| 100M-edge near-full BFS (direction-optimized) | 140 ms |

### Changed

- Workspace upgraded to DataFusion 47 / arrow 55 / parquet 55 (required by
  `iceberg-datafusion` 0.5.1)
- MSRV set to 1.85 (bounded by `iceberg` 0.5.1)

[Unreleased]: https://github.com/Intelligence-Builder/FusionGraph/compare/main...HEAD
[0.1.0]: https://github.com/Intelligence-Builder/FusionGraph/commits/main
