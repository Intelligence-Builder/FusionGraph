//! `GraphTableProvider` implementation.
//!
//! This module provides the `GraphTableProvider` struct that extends `DataFusion`'s
//! `TableProvider` for graph-aware table access. While the API reference describes
//! a trait-based design, the current implementation uses a concrete struct with
//! inherent methods for simplicity and performance.

use std::any::Any;
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use async_trait::async_trait;
use datafusion::catalog::Session;
use datafusion::catalog::TableProvider;
use datafusion::datasource::TableType;
use datafusion::error::Result;
use datafusion::logical_expr::Expr;
use datafusion::physical_plan::ExecutionPlan;

use fusiongraph_core::traversal::TraversalSpec;
use fusiongraph_core::{CsrGraph, GraphStatistics};
use fusiongraph_ontology::{IdType, Ontology};

use crate::error::DataFusionError;
use crate::exec::GraphTraversalExec;

/// A `TableProvider` that exposes graph topology alongside relational data.
#[derive(Debug)]
pub struct GraphTableProvider {
    ontology: Arc<Ontology>,
    graph: Arc<CsrGraph>,
    schema: SchemaRef,
    materialized: bool,
}

impl GraphTableProvider {
    /// Creates a new `GraphTableProvider` with the given ontology.
    #[must_use]
    pub fn new(ontology: Ontology, schema: SchemaRef) -> Self {
        Self {
            ontology: Arc::new(ontology),
            graph: Arc::new(CsrGraph::empty()),
            schema,
            materialized: false,
        }
    }

    /// Returns the ontology schema.
    #[must_use]
    pub fn ontology(&self) -> &Ontology {
        &self.ontology
    }

    /// Returns available node labels.
    #[must_use]
    pub fn node_labels(&self) -> Vec<&str> {
        self.ontology.node_labels()
    }

    /// Returns available edge labels.
    #[must_use]
    pub fn edge_labels(&self) -> Vec<&str> {
        self.ontology.edge_labels()
    }

    /// Returns true if the CSR is materialized.
    #[must_use]
    pub const fn is_materialized(&self) -> bool {
        self.materialized
    }

    /// Returns graph statistics.
    #[must_use]
    pub fn statistics(&self) -> GraphStatistics {
        self.graph.statistics()
    }

    /// Returns a reference to the underlying graph.
    #[must_use]
    pub const fn graph(&self) -> &Arc<CsrGraph> {
        &self.graph
    }

    /// Returns the schema for a specific node type.
    ///
    /// The schema is derived from the ontology's node definition, including
    /// the ID column and any declared properties.
    #[must_use]
    pub fn node_schema(&self, label: &str) -> Option<SchemaRef> {
        let node_def = self.ontology.node(label)?;

        // Build schema from node definition
        let id_type = match self.ontology.settings.default_node_id_type {
            IdType::U32 => DataType::UInt32,
            IdType::U64 => DataType::UInt64,
            IdType::U128 | IdType::String => DataType::Utf8,
        };

        let mut fields = vec![Field::new("node_id", id_type, false)];
        fields.push(Field::new("label", DataType::Utf8, false));

        // Add property fields (as Utf8 for now; actual types come from source tables)
        for prop in &node_def.properties {
            fields.push(Field::new(prop, DataType::Utf8, true));
        }

        Some(Arc::new(Schema::new(fields)))
    }

    /// Returns the schema for a specific edge type.
    ///
    /// The schema is derived from the ontology's edge definition, including
    /// source/target columns and any declared properties.
    #[must_use]
    pub fn edge_schema(&self, label: &str) -> Option<SchemaRef> {
        let edge_def = self.ontology.edge(label)?;

        let id_type = match self.ontology.settings.default_node_id_type {
            IdType::U32 => DataType::UInt32,
            IdType::U64 => DataType::UInt64,
            IdType::U128 | IdType::String => DataType::Utf8,
        };

        let mut fields = vec![
            Field::new("source_id", id_type.clone(), false),
            Field::new("target_id", id_type, false),
            Field::new("label", DataType::Utf8, false),
        ];

        // Add weight column if present
        if edge_def.weight_column.is_some() {
            fields.push(Field::new("weight", DataType::Float64, true));
        }

        // Add property fields
        for prop in &edge_def.properties {
            fields.push(Field::new(prop, DataType::Utf8, true));
        }

        Some(Arc::new(Schema::new(fields)))
    }

    /// Forces materialization of the CSR from underlying tables.
    ///
    /// This method builds the CSR graph structure from the source tables
    /// defined in the ontology. After materialization, `is_materialized()`
    /// returns `true`.
    ///
    /// # Errors
    ///
    /// Returns an error if the CSR build fails.
    #[allow(clippy::unused_async)]
    pub async fn materialize(
        &mut self,
        _state: &dyn Session,
    ) -> std::result::Result<(), DataFusionError> {
        // TODO: Implement actual CSR materialization from source tables.
        // Until that exists, do not report success or mark the graph as materialized.
        Err(DataFusionError::NotImplemented(
            "CSR materialization is not yet implemented".to_string(),
        ))
    }

    /// Creates a traversal execution plan for the graph.
    ///
    /// This method creates a `GraphTraversalExec` physical plan that will
    /// execute the specified traversal when run.
    ///
    /// # Errors
    ///
    /// Returns an error if the graph is not materialized or plan creation fails.
    #[allow(clippy::unused_async)]
    pub async fn create_traversal_plan(
        &self,
        _state: &dyn Session,
        spec: TraversalSpec,
        _filters: &[Expr],
    ) -> std::result::Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        if !self.materialized {
            return Err(DataFusionError::PlanGenerationFailed {
                reason: "Graph must be materialized before traversal".to_string(),
            });
        }

        let exec = GraphTraversalExec::new(Arc::clone(&self.graph), spec);
        Ok(Arc::new(exec))
    }
}

#[async_trait]
impl TableProvider for GraphTableProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        _state: &dyn Session,
        _projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // TODO: Implement graph-aware scanning
        Err(datafusion::error::DataFusionError::NotImplemented(
            "GraphTableProvider::scan not yet implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_schema::{DataType, Field, Schema};
    use fusiongraph_core::traversal::{TraversalAlgorithm, TraversalDirection};

    fn test_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("node_id", DataType::UInt64, false),
            Field::new("label", DataType::Utf8, false),
        ]))
    }

    fn test_ontology() -> Ontology {
        fusiongraph_ontology::parse_toml(
            r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"
properties = ["name", "email"]

[[edges]]
label = "FOLLOWS"
source = "follows"
from_node = "User"
from_column = "follower_id"
to_node = "User"
to_column = "followee_id"
properties = ["since"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn provider_creation() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());

        assert_eq!(provider.node_labels(), vec!["User"]);
        assert!(!provider.is_materialized());
    }

    #[test]
    fn provider_schema() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());
        let schema = provider.schema();

        assert_eq!(schema.fields().len(), 2);
    }

    #[test]
    fn node_schema_returns_correct_fields() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());

        let schema = provider
            .node_schema("User")
            .expect("User schema should exist");
        assert!(schema.field_with_name("node_id").is_ok());
        assert!(schema.field_with_name("label").is_ok());
        assert!(schema.field_with_name("name").is_ok());
        assert!(schema.field_with_name("email").is_ok());
    }

    #[test]
    fn node_schema_returns_none_for_unknown() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());

        assert!(provider.node_schema("Unknown").is_none());
    }

    #[test]
    fn edge_schema_returns_correct_fields() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());

        let schema = provider
            .edge_schema("FOLLOWS")
            .expect("FOLLOWS schema should exist");
        assert!(schema.field_with_name("source_id").is_ok());
        assert!(schema.field_with_name("target_id").is_ok());
        assert!(schema.field_with_name("label").is_ok());
        assert!(schema.field_with_name("since").is_ok());
    }

    #[test]
    fn edge_schema_returns_none_for_unknown() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());

        assert!(provider.edge_schema("UNKNOWN").is_none());
    }

    #[tokio::test]
    async fn create_traversal_plan_requires_materialization() {
        let provider = GraphTableProvider::new(test_ontology(), test_schema());
        let spec = TraversalSpec {
            start: vec![fusiongraph_core::NodeId::new(1)],
            max_depth: 3,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        // Create a mock session - we just need something that implements Session
        let ctx = datafusion::prelude::SessionContext::new();
        let state = ctx.state();

        let result = provider.create_traversal_plan(&state, spec, &[]).await;
        assert!(result.is_err());
    }
}
