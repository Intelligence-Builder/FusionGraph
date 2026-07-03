//! End-to-end zero-ETL demo: Parquet -> CSR projection -> SQL traversal.
//!
//! ```bash
//! cargo run -p fusiongraph-datafusion --example graph_traverse
//! ```
//!
//! Models a small IAM-style permission graph, stores it in Parquet, projects
//! it into a CSR graph with the real operator pipeline, and runs blast-radius
//! queries in plain SQL via the `graph_traverse` table function.

use std::fs::File;
use std::sync::Arc;

use arrow_array::{RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use datafusion::physical_plan::coalesce_partitions::CoalescePartitionsExec;
use datafusion::physical_plan::collect;
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use fusiongraph_datafusion::{
    new_graph_sink, register_graph_traverse, CSRBuilderExec, CsrBuildConfig, GraphCatalog,
};
use parquet::arrow::ArrowWriter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -----------------------------------------------------------------
    // 1. The lakehouse side: an edge table stored in Parquet.
    //    Nodes: 0=admin-role, 1=ci-role, 2=deploy-policy, 3=prod-bucket,
    //           4=dev-bucket, 5=audit-log
    // -----------------------------------------------------------------
    let schema = Arc::new(Schema::new(vec![
        Field::new("source", DataType::UInt64, false),
        Field::new("target", DataType::UInt64, false),
    ]));
    let edges: [(u64, u64); 7] = [
        (0, 2), // admin-role  -> deploy-policy
        (1, 2), // ci-role     -> deploy-policy
        (2, 3), // deploy-policy -> prod-bucket
        (2, 4), // deploy-policy -> dev-bucket
        (0, 5), // admin-role  -> audit-log
        (3, 5), // prod-bucket -> audit-log
        (4, 5), // dev-bucket  -> audit-log
    ];
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(UInt64Array::from(
                edges.iter().map(|&(s, _)| s).collect::<Vec<_>>(),
            )),
            Arc::new(UInt64Array::from(
                edges.iter().map(|&(_, t)| t).collect::<Vec<_>>(),
            )),
        ],
    )?;

    let path = std::env::temp_dir().join("fusiongraph_demo_edges.parquet");
    let mut writer = ArrowWriter::try_new(File::create(&path)?, Arc::clone(&schema), None)?;
    writer.write(&batch)?;
    writer.close()?;
    println!("1. Wrote edge table to {}", path.display());

    // -----------------------------------------------------------------
    // 2. Project Parquet into a CSR graph with the operator pipeline:
    //    ParquetExec -> CoalescePartitionsExec -> CSRBuilderExec.
    // -----------------------------------------------------------------
    let ctx = SessionContext::new();
    ctx.register_parquet(
        "edges",
        path.to_str().expect("utf8 path"),
        ParquetReadOptions::default(),
    )
    .await?;

    let scan = ctx.table("edges").await?.create_physical_plan().await?;
    let coalesced = Arc::new(CoalescePartitionsExec::new(scan));
    let sink = new_graph_sink();
    let builder = Arc::new(CSRBuilderExec::new(
        coalesced,
        CsrBuildConfig {
            graph_sink: Some(Arc::clone(&sink)),
            ..CsrBuildConfig::default()
        },
    ));
    collect(builder, ctx.task_ctx()).await?;
    let graph = Arc::clone(sink.get().expect("graph built"));
    println!(
        "2. Projected CSR graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // -----------------------------------------------------------------
    // 3. Register the graph and the graph_traverse table function.
    // -----------------------------------------------------------------
    let catalog = GraphCatalog::new();
    catalog.register("iam", graph);
    register_graph_traverse(&ctx, &catalog);
    println!("3. Registered graph 'iam' and graph_traverse()");

    // -----------------------------------------------------------------
    // 4. Blast radius in plain SQL: everything reachable from admin-role.
    // -----------------------------------------------------------------
    println!("\n4. Blast radius of admin-role (node 0), 3 hops:\n");
    ctx.sql(
        "SELECT node_id, depth \
         FROM graph_traverse('iam', 0, 3) \
         WHERE depth > 0 \
         ORDER BY depth, node_id",
    )
    .await?
    .show()
    .await?;

    // Traversal results compose with regular SQL: join back to the edge
    // table to count each reached node's own outgoing permissions.
    println!("\n5. Reached nodes joined against the Parquet edge table:\n");
    ctx.sql(
        "SELECT t.node_id, t.depth, COUNT(e.target) AS outgoing_edges \
         FROM graph_traverse('iam', 0, 3) t \
         LEFT JOIN edges e ON e.source = t.node_id \
         GROUP BY t.node_id, t.depth \
         ORDER BY t.depth, t.node_id",
    )
    .await?
    .show()
    .await?;

    Ok(())
}
