//! `FusionGraph` `DataFusion` integration: zero-ETL graph traversal for the
//! lakehouse, embedded in your process.
//!
//! # Embedding guide
//!
//! The crate follows a **build once, traverse many** model. Data stays in
//! Parquet/Iceberg; a one-time projection streams the edge columns into an
//! in-memory CSR graph, and every subsequent traversal runs in microseconds.
//!
//! ## 1. Register your data
//!
//! Any `DataFusion` table works. For Iceberg (feature `iceberg`, default-on):
//!
//! ```ignore
//! use fusiongraph_datafusion::register_iceberg_table_snapshot;
//!
//! // Pin to a snapshot for reproducible, auditable graph builds.
//! register_iceberg_table_snapshot(&ctx, "edges", table, snapshot_id).await?;
//! ```
//!
//! ## 2. Project tables into graphs
//!
//! Declaratively, from a `fusiongraph.toml` ontology:
//!
//! ```ignore
//! use fusiongraph_datafusion::{register_graph_traverse, register_ontology_graphs, GraphCatalog};
//! use fusiongraph_ontology::Ontology;
//!
//! let graphs = GraphCatalog::new();
//! register_graph_traverse(&ctx, &graphs);
//!
//! let ontology = Ontology::from_file("fusiongraph.toml")?;
//! let names = register_ontology_graphs(&ctx, &ontology, &graphs).await?;
//! ```
//!
//! Or imperatively with the operators ([`CSRBuilderExec`] + [`GraphSink`])
//! when you need custom plans — see `examples/graph_traverse.rs`.
//!
//! ## 3. Traverse from SQL
//!
//! ```sql
//! SELECT t.node_id, t.depth, COUNT(e.target) AS out_edges
//! FROM graph_traverse('iam_graph.CAN_ASSUME', 0, 3) t
//! LEFT JOIN edges e ON e.source = t.node_id
//! GROUP BY t.node_id, t.depth;
//! ```
//!
//! Traversal results are ordinary tables: joins, filters, aggregations, and
//! `LIMIT` all compose. Or call the kernel directly from Rust via
//! `fusiongraph_core::traversal::bfs`.
//!
//! # Feature flags
//!
//! | Feature | Default | Effect |
//! |---------|---------|--------|
//! | `iceberg` | yes | `register_iceberg_table` / `register_iceberg_table_snapshot` via the official `iceberg-datafusion` provider (manifest-based file pruning included) |
//!
//! # Components
//!
//! - [`CSRBuilderExec`] — physical operator: `RecordBatch` stream → CSR graph
//! - [`GraphTraversalExec`] — physical operator: BFS as an `ExecutionPlan`
//! - [`GraphCatalog`] / `graph_traverse` — the SQL surface
//! - [`register_ontology_graphs`] — declarative table→graph projection
//! - [`GraphTableProvider`] — ontology-aware `TableProvider`
//!
//! Runnable demos: `cargo run -p fusiongraph-datafusion --example
//! graph_traverse` (Parquet) and `--example iceberg_graph` (Iceberg with
//! snapshot pinning).

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod error;
pub mod exec;
#[cfg(feature = "iceberg")]
pub mod iceberg_ext;
pub mod loader;
pub mod provider;
pub mod udtf;

pub use error::DataFusionError;
pub use exec::{new_graph_sink, CSRBuilderExec, CsrBuildConfig, GraphSink, GraphTraversalExec};
#[cfg(feature = "iceberg")]
pub use iceberg_ext::{register_iceberg_table, register_iceberg_table_snapshot};
pub use loader::{register_ontology_graphs, register_ontology_graphs_as_of};
pub use provider::GraphTableProvider;
pub use udtf::{register_graph_traverse, GraphCatalog};
