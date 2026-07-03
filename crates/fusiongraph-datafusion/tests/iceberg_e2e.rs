//! End-to-end Iceberg integration test (M3).
//!
//! Exercises the full zero-ETL path against a real Iceberg table on local
//! disk: write edges via the Iceberg writer API, commit a snapshot, register
//! the table in `DataFusion`, project it into a CSR graph with the ontology
//! loader, and traverse it from SQL. Also proves snapshot-pinned graph
//! builds: a provider pinned to snapshot N does not see later appends.
#![cfg(feature = "iceberg")]
#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema as ArrowSchema};
use datafusion::prelude::SessionContext;
use fusiongraph_datafusion::{
    register_graph_traverse, register_iceberg_table, register_iceberg_table_snapshot,
    register_ontology_graphs, GraphCatalog,
};
use fusiongraph_ontology::Ontology;
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
use parquet::file::properties::WriterProperties;

mod memory_catalog;
use memory_catalog::TestMemoryCatalog;

/// Iceberg field-ID metadata key expected on Arrow fields by the writer.
const FIELD_ID_KEY: &str = "PARQUET:field_id";

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

fn edge_batch(edges: &[(i64, i64)]) -> RecordBatch {
    RecordBatch::try_new(
        edge_arrow_schema(),
        vec![
            Arc::new(Int64Array::from(
                edges.iter().map(|&(s, _)| s).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                edges.iter().map(|&(_, t)| t).collect::<Vec<_>>(),
            )),
        ],
    )
    .unwrap()
}

/// Creates an empty `edges` Iceberg table in a fresh warehouse.
async fn create_edge_table(catalog: &TestMemoryCatalog) -> Table {
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

    catalog.create_table(&namespace, creation).await.unwrap()
}

/// Writes `edges` as a data file and commits it as a new snapshot.
async fn append_edges(catalog: &TestMemoryCatalog, table: &Table, edges: &[(i64, i64)]) -> Table {
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
    writer.write(edge_batch(edges)).await.unwrap();
    let data_files = writer.close().await.unwrap();

    let tx = Transaction::new(table);
    let mut append = tx.fast_append(None, vec![]).unwrap();
    append.add_data_files(data_files).unwrap();
    let tx = append.apply().await.unwrap();
    tx.commit(catalog).await.unwrap()
}

fn ontology() -> Ontology {
    Ontology::from_toml(
        r#"
[ontology]
name = "lake_graph"

[[nodes]]
label = "Node"
source = "edges"
id_column = "source"

[[edges]]
label = "LINKS"
source = "edges"
from_node = "Node"
from_column = "source"
to_node = "Node"
to_column = "target"
"#,
    )
    .unwrap()
}

async fn count(ctx: &SessionContext, sql: &str) -> i64 {
    let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
    batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .unwrap()
        .value(0)
}

#[tokio::test]
async fn iceberg_table_to_csr_to_sql_traversal() {
    let warehouse = tempfile::tempdir().unwrap();
    let file_io = FileIOBuilder::new_fs_io().build().unwrap();
    let catalog = TestMemoryCatalog::new(file_io, warehouse.path().to_str().unwrap().to_string());

    // 1. Write a chain 0 -> 1 -> 2 -> 3 into a real Iceberg table.
    let table = create_edge_table(&catalog).await;
    let table = append_edges(&catalog, &table, &[(0, 1), (1, 2), (2, 3)]).await;

    // 2. Register it as a DataFusion table (current snapshot).
    let ctx = SessionContext::new();
    register_iceberg_table(&ctx, "edges", table.clone())
        .await
        .unwrap();
    assert_eq!(count(&ctx, "SELECT COUNT(*) FROM edges").await, 3);

    // 3. Project into a CSR graph via the ontology loader and traverse in SQL.
    let graphs = GraphCatalog::new();
    register_graph_traverse(&ctx, &graphs);
    let names = register_ontology_graphs(&ctx, &ontology(), &graphs)
        .await
        .unwrap();
    assert_eq!(names, vec!["lake_graph.LINKS".to_string()]);

    let visited = count(
        &ctx,
        "SELECT COUNT(*) FROM graph_traverse('lake_graph.LINKS', 0, 3)",
    )
    .await;
    assert_eq!(visited, 4, "BFS from 0 reaches {{0, 1, 2, 3}}");
}

#[tokio::test]
async fn snapshot_pinned_graph_builds_are_reproducible() {
    let warehouse = tempfile::tempdir().unwrap();
    let file_io = FileIOBuilder::new_fs_io().build().unwrap();
    let catalog = TestMemoryCatalog::new(file_io, warehouse.path().to_str().unwrap().to_string());

    // Snapshot 1: chain 0 -> 1 -> 2.
    let table = create_edge_table(&catalog).await;
    let table_v1 = append_edges(&catalog, &table, &[(0, 1), (1, 2)]).await;
    let snapshot_v1 = table_v1
        .metadata()
        .current_snapshot()
        .unwrap()
        .snapshot_id();

    // Snapshot 2: extend the chain to 0 -> 1 -> 2 -> 3 -> 4.
    let table_v2 = append_edges(&catalog, &table_v1, &[(2, 3), (3, 4)]).await;

    let ctx = SessionContext::new();
    register_iceberg_table(&ctx, "edges_current", table_v2.clone())
        .await
        .unwrap();
    register_iceberg_table_snapshot(&ctx, "edges", table_v2, snapshot_v1)
        .await
        .unwrap();

    // The pinned table only sees snapshot 1's rows.
    assert_eq!(count(&ctx, "SELECT COUNT(*) FROM edges_current").await, 4);
    assert_eq!(count(&ctx, "SELECT COUNT(*) FROM edges").await, 2);

    // A graph projected from the pinned table is frozen at snapshot 1.
    let graphs = GraphCatalog::new();
    register_graph_traverse(&ctx, &graphs);
    register_ontology_graphs(&ctx, &ontology(), &graphs)
        .await
        .unwrap();

    let visited = count(
        &ctx,
        "SELECT COUNT(*) FROM graph_traverse('lake_graph.LINKS', 0, 10)",
    )
    .await;
    assert_eq!(
        visited, 3,
        "pinned graph reaches only {{0, 1, 2}} despite later appends"
    );
}
