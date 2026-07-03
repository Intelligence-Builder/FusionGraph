//! `FusionGraph` `DataFusion` Integration
//!
//! This crate provides `DataFusion` integration for `FusionGraph`:
//! - `GraphTableProvider` struct implementing `TableProvider`
//! - `CSRBuilderExec` physical operator
//! - `GraphTraversalExec` physical operator
//! - `graph_traverse` SQL table function backed by a [`GraphCatalog`]
//! - Ontology-driven graph registration ([`register_ontology_graphs`])
//! - Optimizer rules for graph query patterns

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
pub use loader::register_ontology_graphs;
pub use provider::GraphTableProvider;
pub use udtf::{register_graph_traverse, GraphCatalog};
