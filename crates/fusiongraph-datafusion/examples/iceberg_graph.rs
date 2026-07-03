//! Zero-ETL graph traversal over an Apache Iceberg table, end to end.
//!
//! ```bash
//! cargo run -p fusiongraph-datafusion --example iceberg_graph
//! ```
//!
//! This example is self-contained: it writes an IAM permission graph into a
//! local-filesystem Iceberg warehouse (via a test-support in-memory catalog),
//! then registers it, projects it, and traverses it in SQL — including a
//! snapshot-pinned build that ignores a later append.
//!
//! ## Production catalogs
//!
//! Swap the test catalog for a real one; everything downstream is identical
//! because [`register_iceberg_table`] only needs an [`iceberg::table::Table`]:
//!
//! ```toml
//! # Cargo.toml
//! iceberg-catalog-rest = "0.5"   # REST catalog (Polaris, Lakekeeper, Unity)
//! # or iceberg-catalog-glue for AWS Glue
//! ```
//!
//! ```rust,ignore
//! use iceberg_catalog_rest::{RestCatalog, RestCatalogConfig};
//!
//! let catalog = RestCatalog::new(
//!     RestCatalogConfig::builder()
//!         .uri("https://my-catalog.example.com".to_string())
//!         .warehouse("s3://my-warehouse".to_string())
//!         .build(),
//! );
//! let table = catalog
//!     .load_table(&TableIdent::from_strs(["lake", "edges"])?)
//!     .await?;
//! register_iceberg_table(&ctx, "edges", table).await?;
//! ```

#[path = "../tests/memory_catalog/mod.rs"]
mod memory_catalog;

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema as ArrowSchema};
use datafusion::prelude::SessionContext;
use fusiongraph_datafusion::{
    register_graph_traverse, register_iceberg_table_snapshot, register_ontology_graphs,
    GraphCatalog,
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
use memory_catalog::TestMemoryCatalog;
use parquet::file::properties::WriterProperties;

const FIELD_ID_KEY: &str = "PARQUET:field_id";

fn edge_batch(edges: &[(i64, i64)]) -> RecordBatch {
    let field = |name: &str, id: &str| {
        Field::new(name, DataType::Int64, false)
            .with_metadata(HashMap::from([(FIELD_ID_KEY.to_string(), id.to_string())]))
    };
    let schema = Arc::new(ArrowSchema::new(vec![
        field("source", "1"),
        field("target", "2"),
    ]));
    RecordBatch::try_new(
        schema,
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
    append.apply().await.unwrap().commit(catalog).await.unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. A local Iceberg warehouse with an `edges` table.
    let warehouse = tempfile::tempdir()?;
    let file_io = FileIOBuilder::new_fs_io().build()?;
    let catalog = TestMemoryCatalog::new(file_io, warehouse.path().to_str().unwrap().to_string());

    let namespace = NamespaceIdent::new("lake".to_string());
    catalog.create_namespace(&namespace, HashMap::new()).await?;
    let schema = Schema::builder()
        .with_fields(vec![
            NestedField::required(1, "source", Type::Primitive(PrimitiveType::Long)).into(),
            NestedField::required(2, "target", Type::Primitive(PrimitiveType::Long)).into(),
        ])
        .build()?;
    let table = catalog
        .create_table(
            &namespace,
            TableCreation::builder()
                .name("edges".to_string())
                .schema(schema)
                .build(),
        )
        .await?;

    // 2. Snapshot 1: the audited IAM permission graph.
    //    0=admin-role, 1=ci-role, 2=deploy-policy, 3=prod-bucket, 4=audit-log
    let table = append_edges(&catalog, &table, &[(0, 2), (1, 2), (2, 3), (3, 4)]).await;
    let audited_snapshot = table.metadata().current_snapshot().unwrap().snapshot_id();
    println!("1. Iceberg table written; audited snapshot = {audited_snapshot}");

    // 3. Snapshot 2: someone grants ci-role direct prod access afterwards.
    let table = append_edges(&catalog, &table, &[(1, 3)]).await;
    println!("2. Later append committed (ci-role -> prod-bucket)");

    // 4. Register PINNED to the audited snapshot: the graph build is
    //    reproducible no matter what lands in the table afterwards.
    let ctx = SessionContext::new();
    register_iceberg_table_snapshot(&ctx, "edges", table, audited_snapshot).await?;

    // 5. Ontology-driven projection + SQL traversal.
    let ontology = Ontology::from_toml(
        r#"
[ontology]
name = "iam"

[[nodes]]
label = "Entity"
source = "edges"
id_column = "source"

[[edges]]
label = "GRANTS"
source = "edges"
from_node = "Entity"
from_column = "source"
to_node = "Entity"
to_column = "target"
"#,
    )?;
    let graphs = GraphCatalog::new();
    register_graph_traverse(&ctx, &graphs);
    let names = register_ontology_graphs(&ctx, &ontology, &graphs).await?;
    println!("3. Projected graphs: {names:?} (from pinned snapshot)");

    println!("\n4. Blast radius of ci-role (node 1) at the audited snapshot:\n");
    ctx.sql(
        "SELECT node_id, depth FROM graph_traverse('iam.GRANTS', 1, 5) \
         WHERE depth > 0 ORDER BY depth, node_id",
    )
    .await?
    .show()
    .await?;
    println!("\n   (the later ci-role -> prod-bucket edge is NOT visible: the");
    println!("    graph is pinned to snapshot {audited_snapshot})");

    Ok(())
}
