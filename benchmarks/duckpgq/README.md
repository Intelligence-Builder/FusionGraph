# DuckPGQ cross-engine comparison

Runs the same k-hop reachability workload as
`cargo bench -p fusiongraph-datafusion --bench parquet_e2e`, on the **same
Parquet file** (1.25M nodes × out-degree 8 = 10M edges, seeded generator),
against DuckDB + the DuckPGQ community extension (SQL/PGQ `GRAPH_TABLE`
syntax).

```bash
brew install duckdb
cargo bench -p fusiongraph-datafusion --bench parquet_e2e -- --quick  # generates the dataset
./run.sh
```

## Results (2026-07-03, Apple Silicon, DuckDB v1.5.4, duckpgq community)

Semantics parity confirmed first: all three formulations agree the 3-hop
reachable set from node 0 has **584 nodes** (DuckPGQ `GRAPH_TABLE`, DuckDB
chained joins, and FusionGraph BFS, which asserts the same count in its
bench suite).

| Path (3-hop, 10M edges) | Time |
|---|---:|
| FusionGraph `graph_traverse` (operator, incl. Arrow output) | **12.4 µs** |
| DuckDB chained self-joins (plain SQL) | ~21 ms |
| DataFusion chained self-joins (plain SQL) | ~27 ms |
| DataFusion recursive CTE | ~93 ms |
| DuckPGQ `GRAPH_TABLE` quantified path `-[e]->{1,3}` | **~135 s** |

Graph/table load: DuckDB Parquet → tables + property graph ≈ 170 ms
(comparable to FusionGraph's 204 ms Parquet → CSR projection).

## Fair-reading notes

- DuckPGQ is a **community extension under active development**; the
  anchored quantified-path pattern (`WHERE a.id = 0` with `{1,3}`) appears
  to be evaluated without pushing the anchor down at this scale — the ~135 s
  is consistent across cold/warm runs and both hop counts, suggesting a
  global computation per query. Other DuckPGQ operations (e.g. its
  specialized shortest-path implementations) may perform very differently.
- DuckDB's *relational* join performance (~21 ms) is excellent and slightly
  ahead of DataFusion's on this query — the cross-engine relational numbers
  validate each other.
- The FusionGraph number amortizes a one-time 204 ms projection; DuckPGQ's
  property-graph creation (~sub-second) is its analogous step. Both engines
  read the identical Parquet file.
- Single machine, single run pair (cold + warm); treat as indicative, not a
  rigorous multi-trial study.
