//! M3 benchmark tier: the zero-ETL pipeline on an Apache Iceberg table.
//!
//! Mirrors `parquet_e2e` on Iceberg-backed storage: 10M edges are written
//! through the Iceberg writer API into a local-warehouse table, then the
//! same three paths are measured:
//!
//!   1. `pipeline_iceberg_to_csr` — `IcebergTableProvider` scan →
//!      `CoalescePartitionsExec` → `CSRBuilderExec` (one-time projection)
//!   2. `csr_bfs` — k-hop BFS on the projected graph
//!   3. `datafusion_sql` — chained self-join reachability on the Iceberg table
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // bench sizes are small and fixed

#[path = "../tests/memory_catalog/mod.rs"]
mod memory_catalog;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arrow_array::{Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema as ArrowSchema};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use datafusion::physical_plan::coalesce_partitions::CoalescePartitionsExec;
use datafusion::physical_plan::collect;
use datafusion::prelude::SessionContext;
use fusiongraph_core::traversal::bfs;
use fusiongraph_core::{CsrGraph, NodeId};
use fusiongraph_datafusion::{
    new_graph_sink, register_iceberg_table, CSRBuilderExec, CsrBuildConfig,
};
use iceberg::io::FileIOBuilder;
use iceberg::spec::{DataFileFormat, NestedField, PrimitiveType, Schema, Type};
use iceberg::table::Table;
use iceberg::transaction::Transaction;
use iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use iceberg::writer::file_writer::ParquetWriterBuilder;
use iceberg::writer::{IcebergWriter, IcebergWriterBuilder};
use iceberg::{Catalog, NamespaceIdent, TableCreation};
use memory_catalog::TestMemoryCatalog;
use parquet::file::properties::WriterProperties;
use tokio::runtime::Runtime;

const NODES: u64 = 1_250_000;
const DEGREE: u64 = 8; // 10M edges
const SEED: u64 = 0x5EED_CAFE;
const FIELD_ID_KEY: &str = "PARQUET:field_id";
/// Rows per written batch (bounds writer memory).
const BATCH_ROWS: u64 = 1_000_000;

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn edge_arrow_schema() -> Arc<ArrowSchema> {
    let field = |name: &str, id: &str| {
        Field::new(name, DataType::Int64, false)
            .with_metadata(HashMap::from([(FIELD_ID_KEY.to_string(), id.to_string())]))
    };
    Arc::new(ArrowSchema::new(vec![
        field("source", "1"),
        field("target", "2"),
    ]))
}

/// Creates the table and appends 10M generated edges as one snapshot.
async fn create_and_load_table(catalog: &TestMemoryCatalog) -> Table {
    let namespace = NamespaceIdent::new("lake".to_string());
    catalog
        .create_namespace(&namespace, HashMap::new())
        .await
        .unwrap();

    let schema = Schema::builder()
        .with_fields(vec![
            NestedField::required(1, "source", Type::Primitive(PrimitiveType::Long)).into(),
            NestedField::required(2, "target", Type::Primitive(PrimitiveType::Long)).into(),
        ])
        .build()
        .unwrap();
    let creation = TableCreation::builder()
        .name("edges".to_string())
        .schema(schema)
        .build();
    let table = catalog.create_table(&namespace, creation).await.unwrap();

    let location_generator = DefaultLocationGenerator::new(table.metadata().clone()).unwrap();
    let file_name_generator = DefaultFileNameGenerator::new(
        "data".to_string(),
        Some(uuid::Uuid::new_v4().to_string()),
        DataFileFormat::Parquet,
    );
    let parquet_writer = ParquetWriterBuilder::new(
        WriterProperties::builder().build(),
        table.metadata().current_schema().clone(),
        table.file_io().clone(),
        location_generator,
        file_name_generator,
    );
    let mut writer = DataFileWriterBuilder::new(parquet_writer, None, 0)
        .build()
        .await
        .unwrap();

    let arrow_schema = edge_arrow_schema();
    let mut seed = SEED;
    let mut written = 0u64;
    let total = NODES * DEGREE;
    let mut src_id = 0u64;
    while written < total {
        let rows = BATCH_ROWS.min(total - written);
        let mut sources = Vec::with_capacity(rows as usize);
        let mut targets = Vec::with_capacity(rows as usize);
        for _ in 0..rows {
            sources.push(src_id as i64);
            targets.push((xorshift(&mut seed) % NODES) as i64);
            written += 1;
            if written % DEGREE == 0 {
                src_id += 1;
            }
        }
        let batch = RecordBatch::try_new(
            Arc::clone(&arrow_schema),
            vec![
                Arc::new(Int64Array::from(sources)),
                Arc::new(Int64Array::from(targets)),
            ],
        )
        .unwrap();
        writer.write(batch).await.unwrap();
    }
    let data_files = writer.close().await.unwrap();

    let tx = Transaction::new(&table);
    let mut append = tx.fast_append(None, vec![]).unwrap();
    append.add_data_files(data_files).unwrap();
    let tx = append.apply().await.unwrap();
    tx.commit(catalog).await.unwrap()
}

async fn build_graph_via_pipeline(ctx: &SessionContext) -> Arc<CsrGraph> {
    let df = ctx.table("edges").await.unwrap().select(vec![
        datafusion::logical_expr::cast(datafusion::logical_expr::col("source"), DataType::UInt64)
            .alias("source"),
        datafusion::logical_expr::cast(datafusion::logical_expr::col("target"), DataType::UInt64)
            .alias("target"),
    ]);
    let scan = df.unwrap().create_physical_plan().await.unwrap();
    let coalesced = Arc::new(CoalescePartitionsExec::new(scan));

    let sink = new_graph_sink();
    let config = CsrBuildConfig {
        graph_sink: Some(Arc::clone(&sink)),
        ..CsrBuildConfig::default()
    };
    let builder = Arc::new(CSRBuilderExec::new(coalesced, config));
    collect(builder, ctx.task_ctx()).await.unwrap();
    Arc::clone(sink.get().expect("graph built"))
}

fn khop_sql(k: u32) -> String {
    assert!((1..=3).contains(&k));
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

fn bench_iceberg_e2e(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let warehouse = tempfile::tempdir().expect("tempdir");
    let file_io = FileIOBuilder::new_fs_io().build().expect("file io");
    let catalog = TestMemoryCatalog::new(file_io, warehouse.path().to_str().unwrap().to_string());

    let table = rt.block_on(create_and_load_table(&catalog));

    let ctx = SessionContext::new();
    rt.block_on(register_iceberg_table(&ctx, "edges", table))
        .unwrap();

    let graph = rt.block_on(build_graph_via_pipeline(&ctx));
    assert_eq!(graph.edge_count() as u64, NODES * DEGREE);

    let mut group = c.benchmark_group("iceberg_e2e_10m_edges");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    group.bench_function("pipeline_iceberg_to_csr", |b| {
        b.iter(|| rt.block_on(build_graph_via_pipeline(black_box(&ctx))));
    });

    for k in [2u32, 3] {
        group.bench_with_input(BenchmarkId::new("csr_bfs", k), &k, |b, &k| {
            b.iter(|| bfs(black_box(&graph), black_box(NodeId::new(0)), black_box(k)));
        });
    }

    for k in [2u32, 3] {
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

criterion_group!(benches, bench_iceberg_e2e);
criterion_main!(benches);
