//! The benchmark that justifies the project.
//!
//! Compares k-hop reachability on identical edge data via:
//!   1. `CsrGraph` + BFS (the `FusionGraph` kernel path)
//!   2. Equivalent `DataFusion` SQL (chained self-joins + UNION DISTINCT)
//!
//! Semantics compared: "distinct nodes reachable from a start node within
//! k hops" (excluding the start node itself on the SQL side; the BFS result
//! includes it — the ±1 does not affect timing comparisons).
//!
//! CSR build cost is benchmarked separately (`csr_build_from_batch`) so the
//! amortization story stays honest: the graph is built once and traversed
//! many times, while the SQL path pays join cost on every query.
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // bench sizes are small and fixed

use std::sync::Arc;

use arrow_array::{RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use fusiongraph_core::traversal::bfs;
use fusiongraph_core::{CsrGraph, NodeId};
use tokio::runtime::Runtime;

/// Deterministic xorshift64 RNG (no external dependency).
fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Uniform random directed graph with fixed out-degree.
fn generate_edges(nodes: u64, degree: u64, mut seed: u64) -> Vec<(u64, u64)> {
    let mut edges = Vec::with_capacity((nodes * degree) as usize);
    for src in 0..nodes {
        for _ in 0..degree {
            edges.push((src, xorshift(&mut seed) % nodes));
        }
    }
    edges
}

fn edges_to_batch(edges: &[(u64, u64)]) -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("src", DataType::UInt64, false),
        Field::new("dst", DataType::UInt64, false),
    ]));
    let src: UInt64Array = edges.iter().map(|&(s, _)| s).collect();
    let dst: UInt64Array = edges.iter().map(|&(_, d)| d).collect();
    RecordBatch::try_new(schema, vec![Arc::new(src), Arc::new(dst)]).expect("valid edge batch")
}

fn make_context(batch: RecordBatch) -> SessionContext {
    let ctx = SessionContext::new();
    let table = MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("mem table");
    ctx.register_table("edges", Arc::new(table))
        .expect("register edges table");
    ctx
}

/// SQL computing distinct nodes reachable within `k` hops of node 0
/// via chained self-joins (the idiomatic relational formulation).
fn khop_sql(k: u32) -> String {
    assert!((1..=3).contains(&k), "benchmark supports 1..=3 hops");
    let mut unions = vec!["SELECT dst AS n FROM edges WHERE src = 0".to_string()];
    if k >= 2 {
        unions.push(
            "SELECT e2.dst AS n FROM edges e1 \
             JOIN edges e2 ON e1.dst = e2.src WHERE e1.src = 0"
                .to_string(),
        );
    }
    if k >= 3 {
        unions.push(
            "SELECT e3.dst AS n FROM edges e1 \
             JOIN edges e2 ON e1.dst = e2.src \
             JOIN edges e3 ON e2.dst = e3.src WHERE e1.src = 0"
                .to_string(),
        );
    }
    format!(
        "SELECT COUNT(*) FROM (SELECT DISTINCT n FROM ({}))",
        unions.join(" UNION ALL ")
    )
}

fn bench_khop(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    // (nodes, degree): 10k/d8 = 80k edges; 100k/d8 = 800k edges.
    for (nodes, degree) in [(10_000u64, 8u64), (100_000, 8)] {
        let edges = generate_edges(nodes, degree, 0x5EED);
        let graph = CsrGraph::from_edges(&edges);
        let ctx = make_context(edges_to_batch(&edges));

        // Sanity: both sides agree on the size of the 3-hop reachable set.
        let sql_count: i64 = rt.block_on(async {
            let batches = ctx
                .sql(&khop_sql(3))
                .await
                .expect("plan")
                .collect()
                .await
                .expect("collect");
            let col = batches[0]
                .column(0)
                .as_any()
                .downcast_ref::<arrow_array::Int64Array>()
                .expect("count column");
            col.value(0)
        });
        let bfs_count = bfs(&graph, NodeId::new(0), 3).node_count() as i64;
        // BFS includes the start node; SQL counts reachable targets only.
        // Node 0 may or may not be its own k-hop successor, so allow ±1.
        assert!(
            (bfs_count - sql_count).abs() <= 1,
            "semantics diverged: bfs={bfs_count} sql={sql_count}"
        );

        let mut group = c.benchmark_group(format!("khop_n{nodes}_d{degree}"));

        for k in [2u32, 3] {
            group.bench_with_input(BenchmarkId::new("csr_bfs", k), &k, |b, &k| {
                b.iter(|| bfs(black_box(&graph), black_box(NodeId::new(0)), black_box(k)));
            });

            let sql = khop_sql(k);
            group.bench_with_input(BenchmarkId::new("datafusion_sql", k), &sql, |b, sql| {
                b.iter(|| {
                    rt.block_on(async {
                        ctx.sql(black_box(sql))
                            .await
                            .expect("plan")
                            .collect()
                            .await
                            .expect("collect")
                    })
                });
            });
        }

        group.finish();
    }
}

/// CSR construction cost from an Arrow batch — the one-time projection price
/// that BFS queries amortize.
fn bench_csr_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("csr_build_from_batch");

    for (nodes, degree) in [(10_000u64, 8u64), (100_000, 8)] {
        let edges = generate_edges(nodes, degree, 0x5EED);
        group.bench_with_input(
            BenchmarkId::new("random", nodes * degree),
            &edges,
            |b, e| b.iter(|| CsrGraph::from_edges(black_box(e))),
        );
    }

    group.finish();
}

criterion_group!(benches, bench_khop, bench_csr_build);
criterion_main!(benches);
