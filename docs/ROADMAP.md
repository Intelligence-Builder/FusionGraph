# FusionGraph Roadmap

**Last updated:** 2026-07-03
**Status:** Committed scope. This document supersedes the phasing in
`FusionGraph_Technical Blueprint.md` and `FusionGraph_Spec Document.md`,
which are retained as vision documents.

---

## 1. Market Assessment (2026-07)

The "zero-ETL graph on the lakehouse" thesis is externally validated:

- **PuppyGraph** (closed-source, commercial) executes the same core idea —
  graph queries on Iceberg/Parquet without ETL. Shipped Databricks Managed
  Iceberg integration (June 2025), published an AMD GraphRAG case study
  (Oct 2025), and is cited in 2026 research as "the current state-of-the-art
  graph compute engine for Lakehouse." A funded competitor executing the
  same idea confirms the problem is real.
- **DuckPGQ** validates the embedded/extension model (SQL/PGQ community
  extension for DuckDB), but is DuckDB-only.
- **Kùzu was archived in October 2025** following Apple's acquisition.
  2026 landscape reviews agree there is no established open-source successor
  for embedded graph analytics.
- **Apache DataFusion has no graph extension.** The community documents
  custom `ExecutionPlan` operators as a first-class extension point and
  cites "a graph traversal operator" as a plausible use case;
  `datafusion-contrib` exists as a distribution channel. The seat is open.

**Positioning (one line):** *the open-source, embeddable, DataFusion-native
graph engine* — vs. PuppyGraph (closed, server product), DuckPGQ
(DuckDB-only), Kùzu (dead).

## 2. Guiding Principles

1. **Benchmark-first.** The entire value proposition rests on one claim:
   CSR projection + traversal beats join/CTE-based multi-hop queries on the
   same engine by a wide margin. Every milestone must keep that claim
   measured and reproducible. No feature merges ahead of the benchmark that
   justifies it.
2. **Ruthless MVP.** The vision docs describe a multi-year product
   ("Virtual Graph Operating System"). The committed scope is a narrow,
   excellent `datafusion-graph`-style extension.
3. **Portable performance.** Development happens on Apple Silicon (no
   AVX-512). Optimization work targets the `SimdBackend` trait with NEON +
   AVX2 first, guided by profiles — never speculative intrinsics.
4. **Distribution over invention.** Target `datafusion-contrib` conventions
   so the project can eventually live in the DataFusion ecosystem rather
   than beside it.

## 3. Milestones

### M0 — Credible benchmark (DONE / maintain)
- [x] Working criterion bench for CSR BFS (`fusiongraph-core/benches/traversal.rs`)
- [x] **Comparative benchmark:** k-hop reachability via `CsrGraph` + BFS vs.
      the equivalent DataFusion SQL (chained self-joins) on identical data
      (`fusiongraph-datafusion/benches/traversal_vs_sql.rs`)
- [x] CSR build cost measured separately so amortization is honest
- [x] Preliminary results published in README (2026-07-03): 35x–558x speedup
      over chained-join SQL at 80k–800k edges; CSR build amortizes in one
      3-hop query. Re-validate at ≥1M edges in M1.

### M1 — Parquet end-to-end (mostly done 2026-07-03)
- [x] **Pipeline gap found and fixed:** `CSRBuilderExec` built the graph but
      dropped it after emitting statistics — the advertised
      `CSRBuilderExec → GraphTraversalExec` pipeline was impossible to wire.
      Added `GraphSink` (`Arc<OnceLock<Arc<CsrGraph>>>`) to `CsrBuildConfig`
      so callers capture the built graph; covered by
      `test_graph_sink_enables_traversal_pipeline`.
- [x] Bench reading edge tables from Parquet via `ParquetExec` →
      `CoalescePartitionsExec` → `CSRBuilderExec` → `GraphTraversalExec`
      (`fusiongraph-datafusion/benches/parquet_e2e.rs`)
- [x] 10M-edge scale (1.25M nodes × d8, uniform random): operator path beats
      chained-join SQL by ~617x (3-hop) / ~1,340x (2-hop); Parquet → CSR
      projection = 208 ms, amortizes in ~4 SQL queries. Results in README.
- [x] Skewed-degree generator (2026-07-03): `fusiongraph_core::gen::rmat`
      with Graph500 parameters (a,b,c,d = 0.57/0.19/0.19/0.05), seeded and
      dependency-free; `gen::uniform` for the baseline shape.
- [x] 100M-edge tier (R-MAT scale 23 × ef 12), opt-in via `FG_BENCH_LARGE=1`:
      near-full-graph BFS in 416 ms (~240M edges/sec examined).
- [x] Recursive-CTE baseline (2026-07-03): DataFusion 47 executes
      `WITH RECURSIVE` natively; added to `traversal_vs_sql` and
      `parquet_e2e`, semantics-asserted equal to BFS. At 10M edges on
      Parquet: 75.2 ms (2-hop) / 93.3 ms (3-hop) — 3–7x slower than
      hand-tuned chained joins, putting the operator ~7,500–9,500x ahead
      of the *idiomatic* SQL formulation.
- [ ] Stretch: DuckPGQ on the same Parquet files (cross-engine harness)

### M2 — SQL surface (core done 2026-07-03)
- [x] `graph_traverse()` table function (UDTF) registered on `SessionContext`:
      `SELECT * FROM graph_traverse('graph_name', start, max_hops [, max_nodes])`.
      Positional literal args (DataFusion UDTFs do not support named args).
      Backed by `GraphCatalog`, a thread-safe named registry of built graphs —
      the natural handoff for the build-once/traverse-many amortization model.
      Supports projection pushdown, `LIMIT` pushdown, `WHERE`, and joins
      against regular tables. 9 tests cover happy paths and arg errors.
- [x] Runnable end-to-end demo (`examples/graph_traverse.rs`): Parquet →
      operator pipeline → `GraphCatalog` → blast-radius SQL with a join back
      to the source table.
- [x] Ontology-driven registration (2026-07-03): `register_ontology_graphs`
      validates the ontology, projects every edge definition through the
      operator pipeline (selective 2-column scan, integer IDs cast to
      `UInt64`), and registers each as `"<ontology>.<edge_label>"` in the
      `GraphCatalog`. 5 tests incl. validation-failure and missing-table paths.
      Deferred within this item: weight columns, temporal validity columns,
      string/UUID ID transforms (clear errors/docs in place).
- [ ] Documented public API for embedding (the "Kùzu gap" audience) —
      example exists; needs a docs.rs-quality module guide

### M3 — Iceberg (core done 2026-07-03)
- [x] **Workspace upgraded DataFusion 45 → 47 / arrow 54 → 55** — required
      because no `iceberg-datafusion` release pairs with DF 45 (0.4 → DF 43,
      0.5.1 → DF 47). Migration was small: `MemoryExec` →
      `MemorySourceConfig::try_new_exec`, new `DisplayFormatType::TreeRender`
      match arms. All tests pass.
- [x] `iceberg` + `iceberg-datafusion` 0.5.1 wired behind a default-on
      `iceberg` feature. `register_iceberg_table` /
      `register_iceberg_table_snapshot` expose Iceberg tables to the session;
      the ontology loader + `graph_traverse` work on top unchanged.
- [x] Manifest-statistics-based file pruning — provided by the official
      `IcebergTableProvider` (filter pushdown prunes data files via manifest
      stats); we deliberately reuse it instead of reimplementing.
- [x] Snapshot-pinned graph builds (graph version = Iceberg snapshot ID) via
      `try_new_from_table_snapshot`; covered by
      `snapshot_pinned_graph_builds_are_reproducible`.
- [x] Hermetic e2e test infra: minimal in-memory Iceberg catalog
      (`tests/memory_catalog/`) with a working `update_table` — upstream's
      0.5.1-era memory catalog lacked commit support and is yanked anyway.
- [x] Iceberg benchmark tier (2026-07-03, `--bench iceberg_e2e`): 10M-edge
      Iceberg-backed projection = 174 ms (vs. 204 ms Parquet); SQL joins on
      the Iceberg provider are ~8x slower than raw Parquet (91.9/220 ms),
      which widens the kernel's advantage to ~17,700x (3-hop via operator).
- [x] Runnable Iceberg example (`--example iceberg_graph`): snapshot-pinned
      audit scenario; REST (Polaris/Lakekeeper/Unity) and Glue catalog wiring
      documented in the example header — swapping the catalog changes nothing
      downstream because the integration only needs an `iceberg::table::Table`.

### M4 — Performance depth (core done 2026-07-03; profile-guided)
- [x] **BFS hot-path rewrite** — the profile said the wins were structural,
      not SIMD: dense `u64` bitset visited tracking (was `HashSet<NodeId>`),
      zero-copy `&[u32]` neighbor slices from CSR storage (was an allocating
      per-node iterator), allocation-free batch filtering. Result: 3-hop BFS
      at 10M edges **91 µs → 6.8 µs (~13x)**; `GraphTraversalExec` now calls
      the shared kernel (its private HashSet BFS is gone).
- [x] `bfs_bounded` kernel API (multi-start + `max_nodes` cap) shared by the
      operator; `bfs_bounded_with_backend` for backend benchmarking.
- [x] NEON implementation of `filter_unvisited` (real intrinsics,
      equivalence-tested against scalar). **Honest finding:** ~5% slower
      than the optimized scalar kernel on Apple Silicon — the operation is
      gather-bound and NEON has no gather — so `select_backend()` returns
      scalar on `aarch64`. Backend stays available for other ARM cores.
- [x] AVX2 implementation (gather + variable shifts + movemask); compiles
      and dispatches behind runtime detection, delegated to by the AVX-512
      slot. **Validated on real x86_64 hardware** via CI (2026-07-03): the
      equivalence tests passed on the ubuntu runner; the first CI run also
      caught an x86-only lint the aarch64 dev machine could not see.
- [x] Delta-layer traversal benchmark: clean fast path 4.9 µs vs. 361 µs
      with 1k delta insertions + 100 tombstones (~74x) — motivates
      compaction (below).
- [x] Delta → base compaction (2026-07-03): `CsrGraph::compact()` —
      LSM-style merge (insertions materialized, tombstones dropped, weights
      preserved, `NeighborIter` dedupe semantics kept). Benchmark:
      377 µs dirty → 3.9 µs compacted — the ~74x slow-path penalty is now
      bounded by a compaction call.
- [x] CSR transpose (2026-07-03): `CsrGraph::transpose()` reverses the full
      merged topology (base + delta), enabling incoming-edge traversal
      ("who can reach X?") as an outgoing BFS on the reverse graph.
      `GraphTraversalExec`'s incoming-direction error now points at it.
- [x] CI (2026-07-03): GitHub Actions on ubuntu (x86_64) + macOS (aarch64);
      the SIMD equivalence tests validate the AVX2 kernel on the x86 runner
      and NEON on the mac runner. fmt + clippy `-D warnings` + tests +
      no-default-features check + bench compilation.
- [x] Direction-optimizing BFS — done under M5 (see below): transpose +
      α/β frontier-density heuristic, 2.2–3.2x on R-MAT hub traversals
- [ ] AVX2/AVX-512 *timing* numbers from dedicated x86_64 hardware
      (CI validates correctness; shared runners are too noisy for criterion)

### M5 — Ecosystem readiness (next)
- [x] Direction-optimizing BFS (2026-07-03): `bfs_direction_optimized`
      implements Beamer-style top-down/bottom-up switching (α=14, β=24)
      over the forward graph + its transpose. Equivalence-tested against
      `bfs` per level on chain/star/diamond/uniform/R-MAT topologies;
      falls back to the delta-aware BFS when the forward graph has live
      mutations; rejects mismatched or delta-carrying transposes
      (FG-TRV-E002). Measured on R-MAT hub traversals:
      **2.2x at 8.4M edges (3-hop), 3.2x at 100M edges (443 ms → 140 ms,
      ~714M edges/sec)**.
- [x] crates.io publishing prep (2026-07-03): all four names verified
      available; keywords/categories/readme/license metadata complete
      (workspace-inherited); per-crate LICENSE copies; internal deps carry
      `version` requirements; MSRV corrected 1.75 → 1.85 (bounded by
      iceberg 0.5.1; DF 47 needs 1.82, arrow 55 needs 1.81); leaf crates
      pass `cargo publish --dry-run` with build verification; CI/license/
      MSRV badges in README; runbook in `docs/RELEASING.md`.
      **Actual publish is a human action** (crates.io account + token) —
      follow the runbook.
- [ ] Propose to `datafusion-contrib` once published (distribution +
      ecosystem credibility — see §2 Guiding Principles #4)
- [x] Recursive-CTE comparative benchmark — see M1 entry (operator is
      ~7,500–9,500x faster than idiomatic `WITH RECURSIVE` at 10M edges)
- [ ] DuckPGQ cross-engine comparison (needs a separate harness; DuckDB is
      too heavy as a dev-dependency)

## 4. Explicitly Deferred (kill list until further notice)

| Item | Reason |
|------|--------|
| AVX-512 intrinsics | Dev machine is Apple Silicon; AVX-512 downclocks on many Intel SKUs; scalar/NEON/AVX2 first, guided by profiles |
| datafusion-tokomak / e-graph optimizer | Upstream is effectively unmaintained; research-grade risk |
| Snowflake Native App / Horizon / SPCS | Distraction before the kernel is proven; revisit post-M3 |
| "ReflexArc" agentic action layer | Out of scope for a query engine MVP; a triggering layer can be built *on top of* results later |
| Substrait serialization | No consumer for it yet |
| Hot/Warm/Cold tiering, OCI packaging | Premature operationalization |

## 5. Risks

1. **Performance proof fails.** If CSR-over-Parquet cannot demonstrably beat
   join-based traversal at realistic scales, the project has no reason to
   exist. Mitigation: M0/M1 benchmarks come before everything else.
2. **Window closes.** PuppyGraph moves fast; the post-Kùzu gap will not stay
   open indefinitely. Being the first *credible open-source* answer matters.
3. **Scope relapse.** The vision documents are seductive. Any PR that
   implements a kill-list item without a benchmark justification should be
   rejected in review.
