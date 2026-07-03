# FusionGraph

**Zero-ETL graph traversal for Apache DataFusion.**

FusionGraph is an open-source (Apache 2.0), embeddable graph execution layer for
[Apache DataFusion](https://datafusion.apache.org/). It projects relational data
(Arrow record batches, Parquet, and Apache Iceberg tables) into an in-memory
CSR (Compressed Sparse Row) graph and executes multi-hop traversals as native
DataFusion `ExecutionPlan` operators, instead of forcing the data through an
ETL pipeline into a separate graph database.

> **Status: early development (v0.1, pre-release).** The core kernel compiles,
> is tested, and is benchmarked, but the project is not yet production-ready.
> See [Project Status](#project-status) for an honest breakdown of what works
> today vs. what is planned. See [docs/ROADMAP.md](docs/ROADMAP.md) for the
> committed scope and the reasoning behind it.

---

## Why

Graph workloads on lakehouse data today require copying data into a dedicated
graph database (Neo4j, TigerGraph, Neptune) and keeping it in sync. The
"query-in-place" alternative is validated and growing — PuppyGraph (commercial,
closed-source) proved the market; DuckPGQ proved the embedded/extension model
for DuckDB; Kùzu's shutdown in late 2025 left the embedded graph analytics
niche without an established open-source successor.

**DataFusion has no graph extension. FusionGraph aims to be it.**

| | PuppyGraph | DuckPGQ | Kùzu | **FusionGraph** |
|---|---|---|---|---|
| License | Closed-source | MIT | Archived (2025) | Apache 2.0 |
| Deployment | Server product | DuckDB extension | Embedded | **Embedded, DataFusion-native** |
| Engine | Proprietary | DuckDB | Own engine | **Apache DataFusion** |
| Language | — | C++ | C++ | **Rust** |

## Target use cases

- **Security / IAM blast-radius analysis** — multi-hop permission traversal over cloud audit tables
- **Fraud / AML** — account → device → merchant → beneficiary path analysis on lakehouse data
- **GraphRAG** — graph-shaped retrieval over data that already lives in Parquet/Iceberg
- **Supply chain & root-cause analysis** — dependency traversal across event and asset tables

## Architecture (current)

```
┌──────────────────────────────────────────────────────┐
│ fusiongraph-datafusion                               │
│   GraphTableProvider (TableProvider impl)            │
│   CSRBuilderExec     (RecordBatch stream → CSR)      │
│   GraphTraversalExec (BFS as an ExecutionPlan)       │
├──────────────────────────────────────────────────────┤
│ fusiongraph-ontology                                 │
│   TOML/JSON schema: tables → node/edge labels        │
├──────────────────────────────────────────────────────┤
│ fusiongraph-core                                     │
│   CSR shards (64MB micro-shards)                     │
│   BFS traversal + visited bitsets                    │
│   Delta layer (DashMap) for edge mutations           │
│   SIMD backend trait (scalar today; see status)      │
├──────────────────────────────────────────────────────┤
│ fusiongraph-ffi                                      │
│   Arrow C Data Interface bindings                    │
└──────────────────────────────────────────────────────┘
```

## Quick start

```bash
cargo build --workspace
cargo test --workspace

# Run benchmarks (includes CSR BFS vs. DataFusion multi-hop join comparison)
cargo bench -p fusiongraph-core
cargo bench -p fusiongraph-datafusion
```

## Benchmarks (preliminary)

k-hop reachability from a start node on a uniform random graph (out-degree 8),
identical data on both paths. SQL path = idiomatic chained self-joins +
`UNION`/`DISTINCT` executed by DataFusion 45 over a `MemTable`. Apple Silicon,
`cargo bench -p fusiongraph-datafusion`, criterion `--quick`:

| Workload | CSR BFS | DataFusion SQL | Speedup |
|---|---:|---:|---:|
| 10k nodes / 80k edges, 2-hop | 11.7 µs | 1.72 ms | **~147x** |
| 10k nodes / 80k edges, 3-hop | 88.9 µs | 3.08 ms | **~35x** |
| 100k nodes / 800k edges, 2-hop | 12.2 µs | 6.81 ms | **~558x** |
| 100k nodes / 800k edges, 3-hop | 88.9 µs | 14.3 ms | **~161x** |

One-time CSR projection cost: 365 µs (80k edges), 3.65 ms (800k edges) — i.e.
building the graph pays for itself within a **single** 3-hop query at 800k-edge
scale. Both paths are sanity-checked to agree on the reachable-set size.

### End-to-end on Parquet and Iceberg, 10M edges (post-M4 kernel)

1.25M nodes × out-degree 8 = 10M edges. The zero-ETL pipeline exercises the
real operators: table scan → `CoalescePartitionsExec` → `CSRBuilderExec`
(graph captured via `GraphSink`) → `GraphTraversalExec`. DataFusion 47:

| Path | 2-hop | 3-hop |
|---|---:|---:|
| CSR BFS (kernel) | 2.5 µs | 6.8 µs |
| `GraphTraversalExec` (operator, incl. Arrow output) | 7.9 µs | 12.4 µs |
| DataFusion SQL, chained joins on **Parquet** | 10.7 ms | 27.2 ms |
| DataFusion SQL, chained joins on **Iceberg** | 91.9 ms | 220 ms |
| **Speedup (operator vs. Parquet SQL)** | **~1,350x** | **~2,190x** |

One-time projection of all 10M edges: **204 ms** from Parquet, **174 ms**
from Iceberg — it pays for itself in a handful of SQL queries. At 100M edges
(R-MAT, skewed degrees), a near-full-graph BFS completes in **416 ms**, and
**direction-optimizing BFS** (`bfs_direction_optimized`, Beamer-style
top-down/bottom-up switching over the graph + its transpose) brings it to
**140 ms (~714M edges/sec)**; run it with
`FG_BENCH_LARGE=1 cargo bench -p fusiongraph-core`.

### M4 kernel notes (profile-guided)

The M4 traversal rewrite (dense visited bitset + zero-copy `&[u32]` neighbor
slices + allocation-free batch filtering) made 3-hop BFS **~13x faster**
(91 µs → 6.8 µs at 10M edges). One honest negative result: hand-written NEON
intrinsics measured **~5% slower** than the optimized scalar kernel on Apple
Silicon — the filter is gather-bound and NEON has no gather instruction — so
`select_backend()` returns scalar on `aarch64` and the NEON/AVX2 backends
remain available for explicit use and re-evaluation on other hardware.

```rust
use fusiongraph_core::{CsrGraph, NodeId, traversal::bfs};

// Build a CSR graph from edges
let graph = CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3)]);

// 3-hop BFS from node 0
let result = bfs(&graph, NodeId::new(0), 3);
assert_eq!(result.node_count(), 4);
```

### Graph traversal in plain SQL

Build a graph once (e.g. from Parquet via the operator pipeline), register it,
and traverse it from any query — results compose with joins, filters, and
aggregations:

```rust
use fusiongraph_datafusion::{register_graph_traverse, GraphCatalog};

let catalog = GraphCatalog::new();
catalog.register("iam", graph); // Arc<CsrGraph> from the build pipeline
register_graph_traverse(&ctx, &catalog);
```

```sql
-- Blast radius: everything reachable from node 0 within 3 hops
SELECT t.node_id, t.depth, COUNT(e.target) AS outgoing_edges
FROM graph_traverse('iam', 0, 3) t
LEFT JOIN edges e ON e.source = t.node_id
WHERE t.depth > 0
GROUP BY t.node_id, t.depth
ORDER BY t.depth;
```

Run the full Parquet → CSR → SQL demo:

```bash
cargo run -p fusiongraph-datafusion --example graph_traverse
```

### Iceberg: ontology-driven, snapshot-pinned graphs

```rust
use fusiongraph_datafusion::{
    register_iceberg_table_snapshot, register_ontology_graphs,
};
use fusiongraph_ontology::Ontology;

// Pin the edge table to an exact Iceberg snapshot: the projected graph is
// reproducible regardless of concurrent appends.
register_iceberg_table_snapshot(&ctx, "edges", table, snapshot_id).await?;

// fusiongraph.toml maps tables -> node/edge labels; every edge definition
// becomes a named graph, immediately queryable via graph_traverse().
let ontology = Ontology::from_file("fusiongraph.toml")?;
let names = register_ontology_graphs(&ctx, &ontology, &catalog).await?;
// e.g. SELECT * FROM graph_traverse('iam_graph.CAN_ASSUME', 0, 3)
```

## Project status

Honest inventory, updated 2026-07.

### Works today
- ✅ CSR storage with micro-sharding (`fusiongraph-core`)
- ✅ BFS traversal with depth tracking and level extraction — M4 kernel:
  dense bitset visited tracking, zero-copy neighbor slices, SIMD backend
  dispatch (scalar/NEON/AVX2, equivalence-tested), delta-aware slow path
- ✅ Deterministic graph generators (`gen::uniform`, `gen::rmat` with
  Graph500 parameters) for reproducible benchmarks
- ✅ Lock-free delta layer for edge insertions/tombstones (DashMap)
- ✅ Ontology schema parser + validation (TOML/JSON)
- ✅ `GraphTableProvider`, `CSRBuilderExec`, `GraphTraversalExec` DataFusion operators
- ✅ `GraphSink`: capture the built graph from `CSRBuilderExec` for downstream traversal
- ✅ `graph_traverse()` SQL table function + `GraphCatalog` registry
  (projection, `WHERE`, `LIMIT`, joins against regular tables all work)
- ✅ Parquet → CSR end-to-end pipeline, benchmarked at 10M edges
- ✅ Arrow C Data Interface FFI surface
- ✅ Criterion benchmarks, including CSR traversal vs. equivalent DataFusion SQL joins
- ✅ Runnable demo: `cargo run -p fusiongraph-datafusion --example graph_traverse`
- ✅ Ontology-driven graph registration: `register_ontology_graphs` projects
  every edge definition in a `fusiongraph.toml` into a named, SQL-queryable graph
- ✅ **Apache Iceberg support** (feature `iceberg`, default-on): register Iceberg
  tables via the official `iceberg-datafusion` provider (manifest-based file
  pruning included) and project them into graphs — with **snapshot-pinned,
  reproducible graph builds** (`register_iceberg_table_snapshot`)

- ✅ Delta → base compaction (`CsrGraph::compact()`): restores the traversal
  fast path after mutations (377 µs dirty → 3.9 µs compacted in benches)
- ✅ CSR transpose (`CsrGraph::transpose()`): incoming-edge traversal
  ("who can reach X?") as outgoing BFS on the reversed topology
- ✅ Direction-optimizing BFS (`bfs_direction_optimized`): Beamer-style
  hybrid traversal, 2.2–3.2x faster on skewed-graph hub traversals
  (100M-edge BFS: 443 ms → 140 ms)
- ✅ CI: GitHub Actions on x86_64 Linux + aarch64 macOS — SIMD kernels
  (AVX2/NEON) equivalence-validated on real hardware every push
- ✅ R-MAT skewed-degree benchmarks incl. opt-in 100M-edge tier
  (`FG_BENCH_LARGE=1`)
- ✅ Iceberg benchmark tier (`--bench iceberg_e2e`) + runnable Iceberg example
  with snapshot pinning (`--example iceberg_graph`) and documented REST/Glue
  catalog wiring
- ✅ docs.rs embedding guide (crate-level docs in `fusiongraph-datafusion`)

### In progress / next (see [ROADMAP](docs/ROADMAP.md))
- 🔜 crates.io publishing + `datafusion-contrib` proposal
- 🔜 DuckPGQ / recursive-CTE comparative benchmarks

### Explicitly deferred (not in MVP scope)
- ⏸️ Hand-written SIMD intrinsics — the `SimdBackend` trait exists with
  runtime dispatch (AVX-512/AVX2/NEON/scalar), but all backends currently
  delegate to the scalar implementation. Vectorization lands only after
  profiling shows it matters on the benchmarked workloads.
- ⏸️ E-graph query optimization (datafusion-tokomak is unmaintained)
- ⏸️ Snowflake Native App / Horizon integration
- ⏸️ Agentic orchestration ("ReflexArc") layer

Documents in `docs/` that describe these deferred features are **vision
documents**, not commitments — each is marked with a status banner.

## Crate structure

| Crate | Purpose |
|-------|---------|
| `fusiongraph-core` | CSR storage, delta layer, BFS traversal |
| `fusiongraph-ontology` | TOML/JSON schema parser and validation |
| `fusiongraph-datafusion` | DataFusion `TableProvider` and `ExecutionPlan` operators |
| `fusiongraph-ffi` | Arrow C Data Interface bindings |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs must pass `cargo test`,
`cargo clippy -- -D warnings`, and `cargo fmt --check`.

## License

Apache 2.0
