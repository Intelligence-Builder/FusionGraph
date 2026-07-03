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

/// A `TableProvider` that exposes graph topology alongside relational data.
///
/// After [`materialize`](Self::materialize), the provider serves the
/// ontology's **merged, governed edge list** as a regular table with schema
/// `(source UInt64, target UInt64, label Utf8)` — one row per edge across
/// every edge definition — while
/// [`create_traversal_plan`](Self::create_traversal_plan) plans traversals
/// over the merged CSR topology.
#[derive(Debug)]
pub struct GraphTableProvider {
    ontology: Arc<Ontology>,
    graph: Arc<CsrGraph>,
    schema: SchemaRef,
    /// Edge-list batches served by `scan` (one per edge definition).
    edge_batches: Vec<arrow_array::RecordBatch>,
    materialized: bool,
}

impl GraphTableProvider {
    /// Creates a new `GraphTableProvider` with the given ontology.
    ///
    /// The table schema is the canonical merged edge list:
    /// `source UInt64, target UInt64, label Utf8`.
    #[must_use]
    pub fn new(ontology: Ontology) -> Self {
        Self {
            ontology: Arc::new(ontology),
            graph: Arc::new(CsrGraph::empty()),
            schema: Self::edge_list_schema(),
            edge_batches: Vec::new(),
            materialized: false,
        }
    }

    /// The canonical merged edge-list schema served by `scan`.
    #[must_use]
    pub fn edge_list_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("source", DataType::UInt64, false),
            Field::new("target", DataType::UInt64, false),
            Field::new("label", DataType::Utf8, false),
        ]))
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
            IdType::U64 | IdType::String => DataType::UInt64,
            IdType::U128 => DataType::FixedSizeBinary(16),
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
            IdType::U64 | IdType::String => DataType::UInt64,
            IdType::U128 => DataType::FixedSizeBinary(16),
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

    /// Materializes the merged CSR graph and edge-list batches from the
    /// ontology's source tables, which must be registered in `ctx`.
    ///
    /// Every edge definition contributes its `(from_column, to_column)`
    /// pairs (cast to `UInt64`) to a single merged topology, tagged per-row
    /// with the edge label in the scannable edge list. After this call,
    /// [`is_materialized`](Self::is_materialized) returns `true` and both
    /// `scan` and [`create_traversal_plan`](Self::create_traversal_plan)
    /// are functional.
    ///
    /// # Errors
    ///
    /// Returns an error if ontology validation fails, a source table or
    /// column is missing, IDs cannot be cast to `UInt64`, or the CSR build
    /// fails.
    pub async fn materialize(
        &mut self,
        ctx: &datafusion::prelude::SessionContext,
    ) -> std::result::Result<(), DataFusionError> {
        use arrow_array::{RecordBatch, StringArray, UInt64Array};
        use datafusion::logical_expr::{cast, col};

        self.ontology
            .validate_or_error()
            .map_err(DataFusionError::Ontology)?;

        let mut all_edges: Vec<(u64, u64)> = Vec::new();
        let mut batches = Vec::with_capacity(self.ontology.edges.len());

        for edge in &self.ontology.edges {
            let df = ctx
                .table(edge.source.as_str())
                .await
                .map_err(DataFusionError::DataFusion)?
                .select(vec![
                    cast(col(edge.from_column.as_str()), DataType::UInt64).alias("source"),
                    cast(col(edge.to_column.as_str()), DataType::UInt64).alias("target"),
                ])
                .map_err(DataFusionError::DataFusion)?;

            let mut sources: Vec<u64> = Vec::new();
            let mut targets: Vec<u64> = Vec::new();
            for batch in df.collect().await.map_err(DataFusionError::DataFusion)? {
                let s = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .ok_or_else(|| DataFusionError::ExecutionFailed {
                        operator: "GraphTableProvider::materialize".to_string(),
                        reason: "source column did not cast to UInt64".to_string(),
                    })?;
                let t = batch
                    .column(1)
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .ok_or_else(|| DataFusionError::ExecutionFailed {
                        operator: "GraphTableProvider::materialize".to_string(),
                        reason: "target column did not cast to UInt64".to_string(),
                    })?;
                for i in 0..batch.num_rows() {
                    if !arrow_array::Array::is_null(s, i) && !arrow_array::Array::is_null(t, i) {
                        sources.push(s.value(i));
                        targets.push(t.value(i));
                    }
                }
            }

            all_edges.extend(sources.iter().copied().zip(targets.iter().copied()));

            let labels: StringArray = std::iter::repeat_n(Some(edge.label.as_str()), sources.len())
                .collect::<Vec<_>>()
                .into();
            let batch = RecordBatch::try_new(
                Self::edge_list_schema(),
                vec![
                    Arc::new(UInt64Array::from(sources)),
                    Arc::new(UInt64Array::from(targets)),
                    Arc::new(labels),
                ],
            )
            .map_err(DataFusionError::Arrow)?;
            batches.push(batch);
        }

        let graph = fusiongraph_core::csr::CsrBuilder::new()
            .with_edges(all_edges)
            .build()
            .map_err(DataFusionError::Graph)?;

        self.graph = Arc::new(graph);
        self.edge_batches = batches;
        self.materialized = true;
        Ok(())
    }

    /// Creates a traversal execution plan over the merged graph.
    ///
    /// Filters are not pushed into the traversal yet; apply them to the
    /// operator's output (`node_id`, `depth`) in the surrounding plan.
    ///
    /// # Errors
    ///
    /// Returns an error if the graph is not materialized.
    pub fn create_traversal_plan(
        &self,
        spec: TraversalSpec,
        _filters: &[Expr],
    ) -> std::result::Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        if !self.materialized {
            return Err(DataFusionError::PlanGenerationFailed {
                reason: "Graph must be materialized before traversal".to_string(),
            });
        }

        Ok(Arc::new(crate::exec::GraphTraversalExec::new(
            Arc::clone(&self.graph),
            spec,
        )))
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
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        limit: Option<usize>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        if !self.materialized {
            return Err(datafusion::error::DataFusionError::Plan(
                "GraphTableProvider is not materialized; call materialize(ctx) first".to_string(),
            ));
        }

        let mut plan: Arc<dyn ExecutionPlan> =
            datafusion::datasource::memory::MemorySourceConfig::try_new_exec(
                std::slice::from_ref(&self.edge_batches),
                Arc::clone(&self.schema),
                projection.cloned(),
            )?;

        if let Some(fetch) = limit {
            plan = Arc::new(datafusion::physical_plan::limit::GlobalLimitExec::new(
                plan,
                0,
                Some(fetch),
            ));
        }

        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_schema::{DataType, Field, Schema};
    use fusiongraph_core::traversal::{TraversalAlgorithm, TraversalDirection};

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

    fn test_ontology_with_id_type(id_type: IdType) -> Ontology {
        let mut ontology = test_ontology();
        ontology.settings.default_node_id_type = id_type;
        ontology
    }

    fn assert_id_fields(schema: &Schema, expected: &DataType) {
        for field_name in ["node_id", "source_id", "target_id"] {
            if let Ok(field) = schema.field_with_name(field_name) {
                assert_eq!(field.data_type(), expected);
            }
        }
    }

    #[test]
    fn provider_creation() {
        let provider = GraphTableProvider::new(test_ontology());

        assert_eq!(provider.node_labels(), vec!["User"]);
        assert!(!provider.is_materialized());
    }

    #[test]
    fn provider_schema() {
        let provider = GraphTableProvider::new(test_ontology());
        let schema = provider.schema();

        // Canonical merged edge list: source, target, label.
        assert_eq!(schema.fields().len(), 3);
        assert_eq!(schema.field(0).name(), "source");
        assert_eq!(schema.field(2).name(), "label");
    }

    #[test]
    fn node_schema_returns_correct_fields() {
        let provider = GraphTableProvider::new(test_ontology());

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
        let provider = GraphTableProvider::new(test_ontology());

        assert!(provider.node_schema("Unknown").is_none());
    }

    #[test]
    fn edge_schema_returns_correct_fields() {
        let provider = GraphTableProvider::new(test_ontology());

        let schema = provider
            .edge_schema("FOLLOWS")
            .expect("FOLLOWS schema should exist");
        assert!(schema.field_with_name("source_id").is_ok());
        assert!(schema.field_with_name("target_id").is_ok());
        assert!(schema.field_with_name("label").is_ok());
        assert!(schema.field_with_name("since").is_ok());
    }

    #[test]
    fn schemas_map_string_ids_to_hashed_uint64() {
        let provider = GraphTableProvider::new(test_ontology_with_id_type(IdType::String));

        let node_schema = provider
            .node_schema("User")
            .expect("User schema should exist");
        let edge_schema = provider
            .edge_schema("FOLLOWS")
            .expect("FOLLOWS schema should exist");

        assert_id_fields(&node_schema, &DataType::UInt64);
        assert_id_fields(&edge_schema, &DataType::UInt64);
    }

    #[test]
    fn schemas_map_u128_ids_to_fixed_size_binary() {
        let provider = GraphTableProvider::new(test_ontology_with_id_type(IdType::U128));

        let node_schema = provider
            .node_schema("User")
            .expect("User schema should exist");
        let edge_schema = provider
            .edge_schema("FOLLOWS")
            .expect("FOLLOWS schema should exist");

        assert_id_fields(&node_schema, &DataType::FixedSizeBinary(16));
        assert_id_fields(&edge_schema, &DataType::FixedSizeBinary(16));
    }

    #[test]
    fn edge_schema_returns_none_for_unknown() {
        let provider = GraphTableProvider::new(test_ontology());

        assert!(provider.edge_schema("UNKNOWN").is_none());
    }

    /// Registers the `follows` source table the test ontology expects.
    fn ctx_with_follows_table() -> datafusion::prelude::SessionContext {
        use arrow_array::{Int64Array, RecordBatch};
        use datafusion::datasource::MemTable;

        let schema = Arc::new(Schema::new(vec![
            Field::new("follower_id", DataType::Int64, false),
            Field::new("followee_id", DataType::Int64, false),
        ]));
        // 1 -> 2 -> 3, plus 1 -> 3.
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![1i64, 2, 1])),
                Arc::new(Int64Array::from(vec![2i64, 3, 3])),
            ],
        )
        .unwrap();
        let ctx = datafusion::prelude::SessionContext::new();
        ctx.register_table(
            "follows",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap()),
        )
        .unwrap();
        ctx
    }

    #[tokio::test]
    async fn materialize_builds_graph_and_edge_list() {
        let mut provider = GraphTableProvider::new(test_ontology());
        let ctx = ctx_with_follows_table();

        provider.materialize(&ctx).await.unwrap();

        assert!(provider.is_materialized());
        assert_eq!(provider.statistics().edge_count, 3);
        assert!(provider.graph().has_edge(
            fusiongraph_core::NodeId::new(1),
            fusiongraph_core::NodeId::new(2)
        ));
    }

    #[tokio::test]
    async fn scan_serves_labeled_edge_list_via_sql() {
        let mut provider = GraphTableProvider::new(test_ontology());
        let ctx = ctx_with_follows_table();
        provider.materialize(&ctx).await.unwrap();

        ctx.register_table("graph_edges", Arc::new(provider))
            .unwrap();
        let batches = ctx
            .sql("SELECT COUNT(*) FROM graph_edges WHERE label = 'FOLLOWS' AND source = 1")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let count = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Int64Array>()
            .unwrap()
            .value(0);
        assert_eq!(count, 2, "node 1 has two outgoing FOLLOWS edges");
    }

    #[tokio::test]
    async fn scan_before_materialize_errors() {
        let provider = GraphTableProvider::new(test_ontology());
        let ctx = datafusion::prelude::SessionContext::new();
        ctx.register_table("graph_edges", Arc::new(provider))
            .unwrap();

        // Planning succeeds (schema is known); execution hits the scan error.
        let err = ctx
            .sql("SELECT * FROM graph_edges")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not materialized"), "got: {err}");
    }

    #[tokio::test]
    async fn create_traversal_plan_requires_materialization() {
        let provider = GraphTableProvider::new(test_ontology());
        let spec = TraversalSpec {
            start: vec![fusiongraph_core::NodeId::new(1)],
            max_depth: 3,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let result = provider.create_traversal_plan(spec, &[]);
        assert!(matches!(
            result,
            Err(DataFusionError::PlanGenerationFailed { .. })
        ));
    }

    #[tokio::test]
    async fn create_traversal_plan_executes_bfs() {
        use datafusion::physical_plan::collect;

        let mut provider = GraphTableProvider::new(test_ontology());
        let ctx = ctx_with_follows_table();
        provider.materialize(&ctx).await.unwrap();

        let spec = TraversalSpec {
            start: vec![fusiongraph_core::NodeId::new(1)],
            max_depth: 3,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };
        let plan = provider.create_traversal_plan(spec, &[]).unwrap();
        let batches = collect(plan, ctx.task_ctx()).await.unwrap();

        let rows: usize = batches.iter().map(arrow_array::RecordBatch::num_rows).sum();
        assert_eq!(rows, 3, "BFS from 1 reaches {{1, 2, 3}}");
    }
}
