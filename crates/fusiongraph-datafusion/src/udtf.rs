//! The `graph_traverse` SQL table function.
//!
//! Exposes graph traversal to plain SQL. Graphs are built once (e.g. via
//! [`CSRBuilderExec`](crate::CSRBuilderExec) with a
//! [`GraphSink`](crate::GraphSink)), registered in a [`GraphCatalog`] under a
//! name, and then traversed from any query:
//!
//! ```sql
//! SELECT node_id, depth
//! FROM graph_traverse('security_graph', 0, 3)
//! WHERE depth > 0
//! ```
//!
//! Arguments (positional, literals):
//! 1. `graph_name` (string) — name registered in the [`GraphCatalog`]
//! 2. `start_node` (integer) — node ID to start the BFS from
//! 3. `max_hops` (integer) — maximum traversal depth
//! 4. `max_nodes` (integer, optional) — cap on visited nodes
//!
//! Output schema matches [`GraphTraversalExec`](crate::GraphTraversalExec):
//! `node_id UInt64, depth UInt32, path List<UInt64>` (path is currently NULL
//! until parent tracking is implemented).

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::catalog::{Session, TableFunctionImpl, TableProvider};
use datafusion::common::ScalarValue;
use datafusion::datasource::TableType;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::Expr;
use datafusion::physical_expr::expressions::col;
use datafusion::physical_plan::limit::GlobalLimitExec;
use datafusion::physical_plan::projection::ProjectionExec;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::SessionContext;

use fusiongraph_core::traversal::TraversalSpec;
use fusiongraph_core::{CsrGraph, NodeId};

use crate::exec::GraphTraversalExec;

/// Thread-safe registry mapping names to built [`CsrGraph`]s.
///
/// This is the handoff point between the build phase (projection from the
/// lakehouse) and the query phase (`graph_traverse` in SQL). Register the
/// catalog once per [`SessionContext`] via [`register_graph_traverse`].
#[derive(Debug, Default)]
pub struct GraphCatalog {
    graphs: RwLock<HashMap<String, Arc<CsrGraph>>>,
}

impl GraphCatalog {
    /// Creates an empty catalog.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers a graph under `name`, returning the previously registered
    /// graph if one existed.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn register(&self, name: impl Into<String>, graph: Arc<CsrGraph>) -> Option<Arc<CsrGraph>> {
        self.graphs
            .write()
            .expect("graph catalog lock poisoned")
            .insert(name.into(), graph)
    }

    /// Returns the graph registered under `name`, if any.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<CsrGraph>> {
        self.graphs
            .read()
            .expect("graph catalog lock poisoned")
            .get(name)
            .map(Arc::clone)
    }

    /// Removes and returns the graph registered under `name`.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn deregister(&self, name: &str) -> Option<Arc<CsrGraph>> {
        self.graphs
            .write()
            .expect("graph catalog lock poisoned")
            .remove(name)
    }

    /// Returns the names of all registered graphs (sorted).
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .graphs
            .read()
            .expect("graph catalog lock poisoned")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }
}

/// Registers the `graph_traverse` table function on a [`SessionContext`].
///
/// Queries resolve graph names against `catalog` at planning time, so graphs
/// registered after this call are still visible to subsequent queries.
pub fn register_graph_traverse(ctx: &SessionContext, catalog: &Arc<GraphCatalog>) {
    ctx.register_udtf(
        "graph_traverse",
        Arc::new(GraphTraverseUdtf {
            catalog: Arc::clone(catalog),
        }),
    );
}

/// [`TableFunctionImpl`] backing the `graph_traverse` SQL function.
#[derive(Debug)]
struct GraphTraverseUdtf {
    catalog: Arc<GraphCatalog>,
}

impl GraphTraverseUdtf {
    fn parse_string(expr: Option<&Expr>, position: &str) -> Result<String> {
        match expr {
            Some(Expr::Literal(ScalarValue::Utf8(Some(s)) | ScalarValue::LargeUtf8(Some(s)))) => {
                Ok(s.clone())
            }
            other => Err(DataFusionError::Plan(format!(
                "graph_traverse: argument {position} must be a string literal, got {other:?}"
            ))),
        }
    }

    fn parse_u64(expr: Option<&Expr>, position: &str) -> Result<u64> {
        let err = |other: &dyn std::fmt::Debug| {
            DataFusionError::Plan(format!(
                "graph_traverse: argument {position} must be a non-negative integer literal, got {other:?}"
            ))
        };
        match expr {
            Some(Expr::Literal(ScalarValue::Int64(Some(v)))) => {
                u64::try_from(*v).map_err(|_| err(&v))
            }
            Some(Expr::Literal(ScalarValue::UInt64(Some(v)))) => Ok(*v),
            Some(Expr::Literal(ScalarValue::Int32(Some(v)))) => {
                u64::try_from(*v).map_err(|_| err(&v))
            }
            Some(Expr::Literal(ScalarValue::UInt32(Some(v)))) => Ok(u64::from(*v)),
            other => Err(err(&other)),
        }
    }
}

impl TableFunctionImpl for GraphTraverseUdtf {
    fn call(&self, args: &[Expr]) -> Result<Arc<dyn TableProvider>> {
        if !(3..=4).contains(&args.len()) {
            return Err(DataFusionError::Plan(format!(
                "graph_traverse expects 3 or 4 arguments \
                 (graph_name, start_node, max_hops [, max_nodes]), got {}",
                args.len()
            )));
        }

        let graph_name = Self::parse_string(args.first(), "1 (graph_name)")?;
        let start_node = Self::parse_u64(args.get(1), "2 (start_node)")?;
        let max_hops = Self::parse_u64(args.get(2), "3 (max_hops)")?;
        let max_nodes = if args.len() == 4 {
            Some(
                usize::try_from(Self::parse_u64(args.get(3), "4 (max_nodes)")?).map_err(|_| {
                    DataFusionError::Plan("graph_traverse: max_nodes exceeds usize".to_string())
                })?,
            )
        } else {
            None
        };

        let graph = self.catalog.get(&graph_name).ok_or_else(|| {
            DataFusionError::Plan(format!(
                "graph_traverse: no graph named '{graph_name}' is registered; \
                 available graphs: {:?}",
                self.catalog.names()
            ))
        })?;

        let max_depth = u32::try_from(max_hops).map_err(|_| {
            DataFusionError::Plan("graph_traverse: max_hops exceeds u32".to_string())
        })?;

        let spec = TraversalSpec {
            start: vec![NodeId::new(start_node)],
            max_depth,
            max_nodes,
            ..TraversalSpec::default()
        };

        Ok(Arc::new(TraversalTable::new(graph, spec)))
    }
}

/// [`TableProvider`] that plans a [`GraphTraversalExec`] for a fixed graph
/// and traversal spec, produced by the `graph_traverse` table function.
#[derive(Debug)]
struct TraversalTable {
    graph: Arc<CsrGraph>,
    spec: TraversalSpec,
    schema: SchemaRef,
}

impl TraversalTable {
    fn new(graph: Arc<CsrGraph>, spec: TraversalSpec) -> Self {
        // Instantiate once to obtain the output schema.
        let schema = GraphTraversalExec::new(Arc::clone(&graph), spec.clone()).schema();
        Self {
            graph,
            spec,
            schema,
        }
    }
}

#[async_trait]
impl TableProvider for TraversalTable {
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
        let mut plan: Arc<dyn ExecutionPlan> = Arc::new(GraphTraversalExec::new(
            Arc::clone(&self.graph),
            self.spec.clone(),
        ));

        if let Some(indices) = projection {
            let exprs = indices
                .iter()
                .map(|&i| {
                    let field = self.schema.field(i);
                    col(field.name(), &self.schema).map(|e| (e, field.name().clone()))
                })
                .collect::<Result<Vec<_>>>()?;
            plan = Arc::new(ProjectionExec::try_new(exprs, plan)?);
        }

        if let Some(fetch) = limit {
            plan = Arc::new(GlobalLimitExec::new(plan, 0, Some(fetch)));
        }

        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{UInt32Array, UInt64Array};

    /// 0 -> 1 -> 2 -> 3, plus 1 -> 3.
    fn test_graph() -> Arc<CsrGraph> {
        Arc::new(CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3), (1, 3)]))
    }

    fn setup() -> (SessionContext, Arc<GraphCatalog>) {
        let ctx = SessionContext::new();
        let catalog = GraphCatalog::new();
        register_graph_traverse(&ctx, &catalog);
        catalog.register("g", test_graph());
        (ctx, catalog)
    }

    #[tokio::test]
    async fn sql_traversal_returns_expected_nodes() {
        let (ctx, _catalog) = setup();

        let batches = ctx
            .sql("SELECT node_id, depth FROM graph_traverse('g', 0, 2) ORDER BY node_id")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let batch = &batches[0];
        // 2-hop BFS from 0: {0, 1, 2, 3} (3 reached via 1 -> 3 at depth 2).
        assert_eq!(batch.num_rows(), 4);

        let node_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(node_ids.values(), &[0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn sql_where_clause_filters_depth() {
        let (ctx, _catalog) = setup();

        let batches = ctx
            .sql("SELECT depth FROM graph_traverse('g', 0, 2) WHERE depth > 0")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let rows: usize = batches.iter().map(arrow_array::RecordBatch::num_rows).sum();
        assert_eq!(rows, 3, "start node (depth 0) should be filtered out");

        for batch in &batches {
            let depths = batch
                .column(0)
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap();
            assert!(depths.iter().flatten().all(|d| d > 0));
        }
    }

    #[tokio::test]
    async fn sql_limit_is_applied() {
        let (ctx, _catalog) = setup();

        let batches = ctx
            .sql("SELECT node_id FROM graph_traverse('g', 0, 2) LIMIT 2")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let rows: usize = batches.iter().map(arrow_array::RecordBatch::num_rows).sum();
        assert_eq!(rows, 2);
    }

    #[tokio::test]
    async fn sql_max_nodes_argument_caps_traversal() {
        let (ctx, _catalog) = setup();

        let batches = ctx
            .sql("SELECT node_id FROM graph_traverse('g', 0, 3, 2)")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let rows: usize = batches.iter().map(arrow_array::RecordBatch::num_rows).sum();
        assert_eq!(rows, 2);
    }

    #[tokio::test]
    async fn unknown_graph_lists_available_names() {
        let (ctx, _catalog) = setup();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('missing', 0, 2)")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no graph named 'missing'"), "got: {err}");
        assert!(err.contains("\"g\""), "should list available graphs: {err}");
    }

    #[tokio::test]
    async fn wrong_argument_count_errors() {
        let (ctx, _catalog) = setup();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('g')")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("expects 3 or 4 arguments"), "got: {err}");
    }

    #[tokio::test]
    async fn non_string_graph_name_errors() {
        let (ctx, _catalog) = setup();

        let err = ctx
            .sql("SELECT * FROM graph_traverse(42, 0, 2)")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be a string literal"), "got: {err}");
    }

    #[tokio::test]
    async fn negative_start_node_errors() {
        let (ctx, _catalog) = setup();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('g', -1, 2)")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be a non-negative integer literal"),
            "got: {err}"
        );
    }

    #[test]
    fn catalog_register_get_deregister() {
        let catalog = GraphCatalog::new();
        assert!(catalog.get("g").is_none());

        assert!(catalog.register("g", test_graph()).is_none());
        assert!(catalog.get("g").is_some());
        assert_eq!(catalog.names(), vec!["g".to_string()]);

        // Re-registering returns the previous graph.
        assert!(catalog.register("g", test_graph()).is_some());

        assert!(catalog.deregister("g").is_some());
        assert!(catalog.get("g").is_none());
        assert!(catalog.names().is_empty());
    }
}
