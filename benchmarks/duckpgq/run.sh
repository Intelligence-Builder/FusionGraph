#!/usr/bin/env bash
# Cross-engine k-hop reachability: DuckPGQ (SQL/PGQ) vs FusionGraph, on the
# SAME Parquet file used by `cargo bench -p fusiongraph-datafusion --bench
# parquet_e2e` (1.25M nodes x out-degree 8 = 10M edges, seeded generator).
#
# Usage:
#   ./run.sh [path/to/edges.parquet]
#
# Prerequisites:
#   - duckdb CLI on PATH (brew install duckdb); the duckpgq community
#     extension is installed on first run
#   - the Parquet dataset (generated automatically by the FusionGraph bench:
#     run `cargo bench -p fusiongraph-datafusion --bench parquet_e2e -- --quick`
#     once, or pass an explicit path)
#
# Methodology:
#   - Graph load (Parquet -> tables -> property graph) is timed separately,
#     mirroring FusionGraph's one-time Parquet -> CSR projection cost
#   - Each query runs twice; the second (warm) timing is what to compare
#   - Semantics parity: the SQL/PGQ result is checked in-engine against a
#     plain chained-join formulation of the same reachability; FusionGraph's
#     equality with those counts is asserted in its own bench suite

set -euo pipefail
cd "$(dirname "$0")"

REPO_ROOT="$(cd ../.. && pwd)"
PARQUET="${1:-$REPO_ROOT/target/tmp/edges_1250000x8.parquet}"

if ! command -v duckdb >/dev/null; then
  echo "error: duckdb CLI not found (brew install duckdb)" >&2
  exit 1
fi
if [ ! -f "$PARQUET" ]; then
  echo "error: dataset not found at $PARQUET" >&2
  echo "generate it: cargo bench -p fusiongraph-datafusion --bench parquet_e2e -- --quick" >&2
  exit 1
fi

echo "== DuckPGQ ($(duckdb --version)) on $PARQUET =="

sed "s|__PARQUET__|$PARQUET|" khop.sql | duckdb

cat <<'EOF'

== FusionGraph reference (same file, same seed; see README) ==
   pipeline_parquet_to_csr (one-time)   ~204 ms
   graph_traverse 2-hop (operator)      ~7.9 us
   graph_traverse 3-hop (operator)      ~12.4 us
Re-measure locally:
   cargo bench -p fusiongraph-datafusion --bench parquet_e2e -- --quick
EOF
