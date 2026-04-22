//! `GraphTableProvider` implementation.

use std::any::Any;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::catalog::Session;
use datafusion::catalog::TableProvider;
use datafusion::datasource::TableType;
use datafusion::error::Result;
use datafusion::logical_expr::Expr;
use datafusion::physical_plan::ExecutionPlan;

use fusiongraph_core::{CsrGraph, GraphStatistics};
use fusiongraph_ontology::Ontology;

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
}
