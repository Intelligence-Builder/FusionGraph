//! M1 benchmark: end-to-end zero-ETL path on Parquet at 10M edges.
//!
//! Exercises the real operator pipeline on a 10M-edge / 1.25M-node uniform
//! random graph stored in Parquet:
//!
//!   1. `pipeline_parquet_to_csr` — `ParquetExec` scan → `CoalescePartitionsExec`
//!      → `CSRBuilderExec` (with graph sink): the one-time projection cost.
//!   2. `csr_bfs` — k-hop BFS on the projected graph (kernel path).
//!   3. `graph_traversal_exec` — same traversal through the `DataFusion`
//!      operator, including Arrow result materialization.
//!   4. `datafusion_sql` — equivalent k-hop reachability via chained
//!      self-joins reading the same Parquet file.
//!
//! The Parquet file is generated once and cached in `CARGO_TARGET_TMPDIR`.
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // bench sizes are small and fixed

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arrow_array::{RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use datafusion::physical_plan::coalesce_partitions::CoalescePartitionsExec;
use datafusion::physical_plan::collect;
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use fusiongraph_core::traversal::{bfs, TraversalSpec};
use fusiongraph_core::{CsrGraph, NodeId};
use fusiongraph_datafusion::{new_graph_sink, CSRBuilderExec, CsrBuildConfig, GraphTraversalExec};
use parquet::arrow::ArrowWriter;
use tokio::runtime::Runtime;

/// 1.25M nodes x out-degree 8 = 10M edges.
const NODES: u64 = 1_250_000;
const DEGREE: u64 = 8;
const SEED: u64 = 0x5EED_CAFE;
/// Edges per generated record batch (keeps peak memory bounded).
const BATCH_NODES: u64 = 125_000;

const fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn edge_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("source", DataType::UInt64, false),
        Field::new("target", DataType::UInt64, false),
    ]))
}

/// Generates the edge Parquet file once; reuses it across bench runs.
fn ensure_parquet() -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("edges_{NODES}x{DEGREE}.parquet"));
    if path.exists() {
        return path;
    }

    let schema = edge_schema();
    let file = File::create(&path).expect("create parquet file");
    let mut writer = ArrowWriter::try_new(file, Arc::clone(&schema), None).expect("arrow writer");

    let mut seed = SEED;
    let mut chunk_start = 0u64;
    while chunk_start < NODES {
        let chunk_end = (chunk_start + BATCH_NODES).min(NODES);
        let rows = ((chunk_end - chunk_start) * DEGREE) as usize;
        let mut sources = Vec::with_capacity(rows);
        let mut targets = Vec::with_capacity(rows);
        for src in chunk_start..chunk_end {
            for _ in 0..DEGREE {
                sources.push(src);
                targets.push(xorshift(&mut seed) % NODES);
            }
        }
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(UInt64Array::from(sources)),
                Arc::new(UInt64Array::from(targets)),
            ],
        )
        .expect("edge batch");
        writer.write(&batch).expect("write batch");
        chunk_start = chunk_end;
    }
    writer.close().expect("close writer");
    path
}

async fn make_context(path: &Path) -> SessionContext {
    let ctx = SessionContext::new();
    ctx.register_parquet(
        "edges",
        path.to_str().expect("utf8 path"),
        ParquetReadOptions::default(),
    )
    .await
    .expect("register parquet");
    ctx
}

/// The full zero-ETL projection: Parquet scan -> coalesce -> `CSRBuilderExec`.
async fn build_graph_via_pipeline(ctx: &SessionContext) -> Arc<CsrGraph> {
    let df = ctx.table("edges").await.expect("edges table");
    let scan = df.create_physical_plan().await.expect("physical plan");
    let coalesced = Arc::new(CoalescePartitionsExec::new(scan));

    let sink = new_graph_sink();
    let config = CsrBuildConfig {
        graph_sink: Some(Arc::clone(&sink)),
        ..CsrBuildConfig::default()
    };
    let builder = Arc::new(CSRBuilderExec::new(coalesced, config));

    let _stats = collect(builder, ctx.task_ctx()).await.expect("build");
    Arc::clone(sink.get().expect("sink holds graph"))
}

/// Distinct nodes reachable within `k` hops of node 0 via chained self-joins.
fn khop_sql(k: u32) -> String {
    assert!((1..=3).contains(&k), "benchmark supports 1..=3 hops");
    let mut unions = vec!["SELECT target AS n FROM edges WHERE source = 0".to_string()];
    if k >= 2 {
        unions.push(
            "SELECT e2.target AS n FROM edges e1 \
             JOIN edges e2 ON e1.target = e2.source WHERE e1.source = 0"
                .to_string(),
        );
    }
    if k >= 3 {
        unions.push(
            "SELECT e3.target AS n FROM edges e1 \
             JOIN edges e2 ON e1.target = e2.source \
             JOIN edges e3 ON e2.target = e3.source WHERE e1.source = 0"
                .to_string(),
        );
    }
    format!(
        "SELECT COUNT(*) FROM (SELECT DISTINCT n FROM ({}))",
        unions.join(" UNION ALL ")
    )
}

async fn run_sql_count(ctx: &SessionContext, sql: &str) -> i64 {
    let batches = ctx
        .sql(sql)
        .await
        .expect("plan")
        .collect()
        .await
        .expect("collect");
    batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .expect("count column")
        .value(0)
}

fn bench_parquet_e2e(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let path = ensure_parquet();
    let ctx = rt.block_on(make_context(&path));

    // Build once for the traversal benches; also proves the pipeline works.
    let graph = rt.block_on(build_graph_via_pipeline(&ctx));
    assert_eq!(graph.edge_count() as u64, NODES * DEGREE);

    // Sanity: kernel and SQL agree on the 3-hop reachable-set size (±1 for
    // the start node, which BFS includes and the SQL formulation does not).
    let sql_count = rt.block_on(run_sql_count(&ctx, &khop_sql(3)));
    let bfs_count = bfs(&graph, NodeId::new(0), 3).node_count() as i64;
    assert!(
        (bfs_count - sql_count).abs() <= 1,
        "semantics diverged: bfs={bfs_count} sql={sql_count}"
    );

    let mut group = c.benchmark_group("parquet_e2e_10m_edges");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    // 1. One-time projection cost: Parquet -> CSR via the real operators.
    group.bench_function("pipeline_parquet_to_csr", |b| {
        b.iter(|| rt.block_on(build_graph_via_pipeline(black_box(&ctx))));
    });

    // 2. Kernel traversal on the projected graph.
    for k in [2u32, 3] {
        group.bench_with_input(BenchmarkId::new("csr_bfs", k), &k, |b, &k| {
            b.iter(|| bfs(black_box(&graph), black_box(NodeId::new(0)), black_box(k)));
        });
    }

    // 3. Traversal through the DataFusion operator (includes Arrow output).
    for k in [2u32, 3] {
        group.bench_with_input(BenchmarkId::new("graph_traversal_exec", k), &k, |b, &k| {
            b.iter(|| {
                let spec = TraversalSpec {
                    start: vec![NodeId::new(0)],
                    max_depth: k,
                    ..TraversalSpec::default()
                };
                let exec = Arc::new(GraphTraversalExec::new(Arc::clone(&graph), spec));
                rt.block_on(collect(exec, ctx.task_ctx())).expect("collect")
            });
        });
    }

    // 4. The relational baseline on the same Parquet data.
    for k in [2u32, 3] {
        let sql = khop_sql(k);
        group.bench_with_input(BenchmarkId::new("datafusion_sql", k), &sql, |b, sql| {
            b.iter(|| rt.block_on(run_sql_count(black_box(&ctx), black_box(sql))));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_parquet_e2e);
criterion_main!(benches);
