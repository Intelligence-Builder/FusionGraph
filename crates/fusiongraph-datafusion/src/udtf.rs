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
//! 2. `start_node` (integer, or string for dictionary-keyed graphs) — node
//!    to start the BFS from; string keys resolve through the graph's
//!    [`NodeDictionary`](crate::dictionary::NodeDictionary)
//! 3. `max_hops` (integer) — maximum traversal depth
//! 4. `max_nodes` (integer, optional) — cap on visited nodes
//! 5. `direction` (string, optional): `'out'` (default) or `'in'`. `'in'`
//!    traverses **incoming** edges ("who can reach X?") over the memoized
//!    transpose (built on first use). A string in position 4 is accepted as
//!    the direction, so `graph_traverse('g', 0, 3, 'in')` works without a
//!    `max_nodes`.
//!
//! Output schema matches [`GraphTraversalExec`](crate::GraphTraversalExec):
//! `node_id UInt64, depth UInt32, path List<UInt64>` (path is currently NULL
//! until parent tracking is implemented).

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

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

use fusiongraph_core::traversal::{TraversalDirection, TraversalSpec};
use fusiongraph_core::{CompactionPolicy, CsrGraph, NodeId};

use crate::exec::GraphTraversalExec;

/// Thread-safe registry mapping names to built [`CsrGraph`]s.
///
/// This is the handoff point between the build phase (projection from the
/// lakehouse) and the query phase (`graph_traverse` in SQL). Register the
/// catalog once per [`SessionContext`] via [`register_graph_traverse`].
#[derive(Debug, Default)]
pub struct GraphCatalog {
    graphs: RwLock<HashMap<String, Arc<GraphEntry>>>,
}

/// A registered graph plus its lazily-built, memoized transpose and an
/// optional node-key dictionary (string-keyed graphs).
#[derive(Debug)]
struct GraphEntry {
    forward: Arc<CsrGraph>,
    reverse: OnceLock<Arc<CsrGraph>>,
    dictionary: Option<Arc<crate::dictionary::NodeDictionary>>,
}

impl GraphEntry {
    fn new(forward: Arc<CsrGraph>) -> Arc<Self> {
        Arc::new(Self {
            forward,
            reverse: OnceLock::new(),
            dictionary: None,
        })
    }
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
            .insert(name.into(), GraphEntry::new(graph))
            .map(|e| Arc::clone(&e.forward))
    }

    /// Registers a graph together with its pre-built transpose, enabling
    /// direction-optimizing BFS and incoming traversal without the lazy
    /// transpose cost on first use.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn register_with_reverse(
        &self,
        name: impl Into<String>,
        forward: Arc<CsrGraph>,
        reverse: Arc<CsrGraph>,
    ) {
        let entry = GraphEntry::new(forward);
        let _ = entry.reverse.set(reverse);
        self.graphs
            .write()
            .expect("graph catalog lock poisoned")
            .insert(name.into(), entry);
    }

    /// Registers a string-keyed graph together with its node-key
    /// dictionary, enabling string start nodes in `graph_traverse` and the
    /// `graph_nodes` table function.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn register_with_dictionary(
        &self,
        name: impl Into<String>,
        forward: Arc<CsrGraph>,
        dictionary: Arc<crate::dictionary::NodeDictionary>,
    ) {
        let entry = Arc::new(GraphEntry {
            forward,
            reverse: OnceLock::new(),
            dictionary: Some(dictionary),
        });
        self.graphs
            .write()
            .expect("graph catalog lock poisoned")
            .insert(name.into(), entry);
    }

    /// Returns the node-key dictionary for `name`, if the graph was
    /// registered with one.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    #[must_use]
    pub fn dictionary(&self, name: &str) -> Option<Arc<crate::dictionary::NodeDictionary>> {
        self.entry(name).and_then(|e| e.dictionary.clone())
    }

    fn entry(&self, name: &str) -> Option<Arc<GraphEntry>> {
        self.graphs
            .read()
            .expect("graph catalog lock poisoned")
            .get(name)
            .map(Arc::clone)
    }

    /// Returns the transposed graph for `name`, building and memoizing it
    /// on first use. Returns `None` if no graph is registered under `name`.
    ///
    /// The transpose snapshots live delta mutations at build time; graphs
    /// mutated afterwards should be re-registered (or compacted via
    /// [`Self::compact_if_needed`], which resets the memoized transpose).
    ///
    /// # Errors
    ///
    /// Returns an error if building the transpose fails (CSR capacity).
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn reverse(&self, name: &str) -> Result<Option<Arc<CsrGraph>>> {
        let Some(entry) = self.entry(name) else {
            return Ok(None);
        };
        if let Some(rev) = entry.reverse.get() {
            return Ok(Some(Arc::clone(rev)));
        }
        // Build outside any lock; a concurrent builder may win the race,
        // in which case our copy is dropped.
        let built = Arc::new(
            entry
                .forward
                .transpose()
                .map_err(|e| DataFusionError::External(Box::new(e)))?,
        );
        let _ = entry.reverse.set(Arc::clone(&built));
        Ok(Some(Arc::clone(entry.reverse.get().unwrap_or(&built))))
    }

    /// Compacts the named graph if `policy` says its delta layer has grown
    /// too large, atomically swapping the registry entry (which also resets
    /// the memoized transpose). Returns whether compaction ran.
    ///
    /// Mutations that land in the old graph's delta *during* compaction are
    /// replayed into the new graph's delta before the swap, so writers
    /// racing a compaction lose no data (they may observe the old graph
    /// briefly after the swap if they hold a stale `Arc`).
    ///
    /// # Errors
    ///
    /// Returns an error if compaction fails (CSR capacity).
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    pub fn compact_if_needed(&self, name: &str, policy: &CompactionPolicy) -> Result<bool> {
        let Some(entry) = self.entry(name) else {
            return Ok(false);
        };
        if !entry.forward.should_compact(policy) {
            return Ok(false);
        }

        let compacted = entry
            .forward
            .compact()
            .map_err(|e| DataFusionError::External(Box::new(e)))?;

        // Replay mutations that raced the compaction into the new delta.
        for ((from, to), data) in entry.forward.delta().drain_insertions() {
            compacted.delta().insert(from, to, data);
        }
        for (from, to) in entry.forward.delta().drain_deletions() {
            compacted.delta().delete(from, to);
        }

        // Node identity is unchanged by compaction: the dictionary carries
        // over. The memoized transpose is intentionally reset (stale).
        let new_entry = Arc::new(GraphEntry {
            forward: Arc::new(compacted),
            reverse: OnceLock::new(),
            dictionary: entry.dictionary.clone(),
        });
        self.graphs
            .write()
            .expect("graph catalog lock poisoned")
            .insert(name.to_string(), new_entry);
        Ok(true)
    }

    /// Returns the graph registered under `name`, if any.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (a writer panicked).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<CsrGraph>> {
        self.entry(name).map(|e| Arc::clone(&e.forward))
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
            .map(|e| Arc::clone(&e.forward))
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
    ctx.register_udtf(
        "graph_nodes",
        Arc::new(GraphNodesUdtf {
            catalog: Arc::clone(catalog),
        }),
    );
}

/// `graph_nodes('name')`: serves a dictionary-keyed graph's
/// `(node_id UInt64, node_key Utf8)` mapping as a table, so traversal
/// results join back to original keys:
///
/// ```sql
/// SELECT k.node_key, t.depth
/// FROM graph_traverse('g', 'alice', 3) t
/// JOIN graph_nodes('g') k ON k.node_id = t.node_id
/// ```
#[derive(Debug)]
struct GraphNodesUdtf {
    catalog: Arc<GraphCatalog>,
}

impl TableFunctionImpl for GraphNodesUdtf {
    fn call(&self, args: &[Expr]) -> Result<Arc<dyn TableProvider>> {
        if args.len() != 1 {
            return Err(DataFusionError::Plan(format!(
                "graph_nodes expects 1 argument (graph_name), got {}",
                args.len()
            )));
        }
        let name = GraphTraverseUdtf::parse_string(args.first(), "1 (graph_name)")?;
        let dictionary = self.catalog.dictionary(&name).ok_or_else(|| {
            DataFusionError::Plan(format!(
                "graph_nodes: graph '{name}' has no node-key dictionary (only \
                 graphs projected from string-keyed ontologies have one); \
                 available graphs: {:?}",
                self.catalog.names()
            ))
        })?;
        let batch = dictionary
            .to_batch()
            .map_err(|e| DataFusionError::ArrowError(e, None))?;
        let table = datafusion::datasource::MemTable::try_new(
            crate::dictionary::NodeDictionary::schema(),
            vec![vec![batch]],
        )?;
        Ok(Arc::new(table))
    }
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
        if !(3..=5).contains(&args.len()) {
            return Err(DataFusionError::Plan(format!(
                "graph_traverse expects 3 to 5 arguments \
                 (graph_name, start_node, max_hops [, max_nodes] [, direction]), got {}",
                args.len()
            )));
        }

        let graph_name = Self::parse_string(args.first(), "1 (graph_name)")?;

        // Start node: integer, or a string key resolved via the graph's
        // node dictionary.
        let start_node = if matches!(
            args.get(1),
            Some(Expr::Literal(
                ScalarValue::Utf8(Some(_)) | ScalarValue::LargeUtf8(Some(_))
            ))
        ) {
            let key = Self::parse_string(args.get(1), "2 (start_node)")?;
            let dictionary = self.catalog.dictionary(&graph_name).ok_or_else(|| {
                DataFusionError::Plan(format!(
                    "graph_traverse: start node '{key}' is a string but graph \
                     '{graph_name}' has no node-key dictionary; use an integer \
                     node ID or project the graph from a string-keyed ontology"
                ))
            })?;
            dictionary.id_of(&key).ok_or_else(|| {
                DataFusionError::Plan(format!(
                    "graph_traverse: node key '{key}' not found in graph '{graph_name}'"
                ))
            })?
        } else {
            Self::parse_u64(args.get(1), "2 (start_node)")?
        };
        let max_hops = Self::parse_u64(args.get(2), "3 (max_hops)")?;

        // Position 4 is `max_nodes` (integer) or `direction` (string).
        let mut max_nodes = None;
        let mut direction_arg: Option<String> = None;
        if let Some(fourth) = args.get(3) {
            if matches!(
                fourth,
                Expr::Literal(ScalarValue::Utf8(_) | ScalarValue::LargeUtf8(_))
            ) {
                direction_arg = Some(Self::parse_string(Some(fourth), "4 (direction)")?);
            } else {
                max_nodes = Some(
                    usize::try_from(Self::parse_u64(Some(fourth), "4 (max_nodes)")?).map_err(
                        |_| {
                            DataFusionError::Plan(
                                "graph_traverse: max_nodes exceeds usize".to_string(),
                            )
                        },
                    )?,
                );
            }
        }
        if let Some(fifth) = args.get(4) {
            if direction_arg.is_some() {
                return Err(DataFusionError::Plan(
                    "graph_traverse: direction was already given in position 4".to_string(),
                ));
            }
            direction_arg = Some(Self::parse_string(Some(fifth), "5 (direction)")?);
        }

        let direction = match direction_arg.as_deref() {
            None | Some("out" | "outgoing") => TraversalDirection::Outgoing,
            Some("in" | "incoming") => TraversalDirection::Incoming,
            Some(other) => {
                return Err(DataFusionError::Plan(format!(
                    "graph_traverse: direction must be 'out' or 'in', got '{other}'"
                )));
            }
        };

        let graph = self.catalog.get(&graph_name).ok_or_else(|| {
            DataFusionError::Plan(format!(
                "graph_traverse: no graph named '{graph_name}' is registered; \
                 available graphs: {:?}",
                self.catalog.names()
            ))
        })?;

        // Incoming traversal needs the transpose (built + memoized on first
        // use). Outgoing traversal picks it up opportunistically if it is
        // already resident, enabling direction-optimizing BFS for free.
        let reverse = if direction == TraversalDirection::Incoming {
            self.catalog.reverse(&graph_name)?
        } else {
            self.catalog
                .entry(&graph_name)
                .and_then(|e| e.reverse.get().map(Arc::clone))
        };

        let max_depth = u32::try_from(max_hops).map_err(|_| {
            DataFusionError::Plan("graph_traverse: max_hops exceeds u32".to_string())
        })?;

        let spec = TraversalSpec {
            start: vec![NodeId::new(start_node)],
            max_depth,
            max_nodes,
            direction,
            ..TraversalSpec::default()
        };

        Ok(Arc::new(TraversalTable::new(graph, reverse, spec)))
    }
}

/// [`TableProvider`] that plans a [`GraphTraversalExec`] for a fixed graph
/// and traversal spec, produced by the `graph_traverse` table function.
#[derive(Debug)]
struct TraversalTable {
    graph: Arc<CsrGraph>,
    reverse: Option<Arc<CsrGraph>>,
    spec: TraversalSpec,
    schema: SchemaRef,
}

impl TraversalTable {
    fn new(graph: Arc<CsrGraph>, reverse: Option<Arc<CsrGraph>>, spec: TraversalSpec) -> Self {
        // Instantiate once to obtain the output schema.
        let schema = GraphTraversalExec::new(Arc::clone(&graph), spec.clone()).schema();
        Self {
            graph,
            reverse,
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
        let mut exec = GraphTraversalExec::new(Arc::clone(&self.graph), self.spec.clone());
        if let Some(reverse) = &self.reverse {
            exec = exec.with_reverse(Arc::clone(reverse));
        }
        let mut plan: Arc<dyn ExecutionPlan> = Arc::new(exec);

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
        assert!(err.contains("expects 3 to 5 arguments"), "got: {err}");
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

    #[tokio::test]
    async fn sql_incoming_direction_finds_ancestors() {
        let (ctx, _catalog) = setup();

        // Who can reach node 3? In 0 -> 1 -> 2 -> 3 (+1 -> 3): everyone.
        let batches = ctx
            .sql("SELECT node_id, depth FROM graph_traverse('g', 3, 5, 'in') ORDER BY node_id")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 4);
        let node_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(node_ids.values(), &[0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn sql_direction_as_fifth_argument_with_max_nodes() {
        let (ctx, _catalog) = setup();

        let batches = ctx
            .sql("SELECT node_id FROM graph_traverse('g', 3, 5, 2, 'in')")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let rows: usize = batches.iter().map(arrow_array::RecordBatch::num_rows).sum();
        assert_eq!(rows, 2, "max_nodes caps the incoming traversal");
    }

    #[tokio::test]
    async fn sql_invalid_direction_errors() {
        let (ctx, _catalog) = setup();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('g', 0, 2, 'sideways')")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("direction must be 'out' or 'in'"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn outgoing_result_is_unchanged_with_resident_reverse() {
        // With the transpose resident, outgoing queries may use DO-BFS;
        // results must be identical to the plain kernel.
        let (ctx, catalog) = setup();
        let plain = ctx
            .sql("SELECT node_id FROM graph_traverse('g', 0, 2) ORDER BY node_id")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        // Force the transpose to become resident.
        catalog.reverse("g").unwrap().unwrap();
        let hybrid = ctx
            .sql("SELECT node_id FROM graph_traverse('g', 0, 2) ORDER BY node_id")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        assert_eq!(plain[0].num_rows(), hybrid[0].num_rows());
        assert_eq!(
            plain[0]
                .column(0)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .values(),
            hybrid[0]
                .column(0)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .values()
        );
    }

    #[test]
    fn register_with_reverse_prebuilds_transpose() {
        let catalog = GraphCatalog::new();
        let graph = test_graph();
        let reverse = Arc::new(graph.transpose().unwrap());
        catalog.register_with_reverse("g", graph, reverse);

        // Available without lazy construction.
        let entry = catalog.entry("g").unwrap();
        assert!(entry.reverse.get().is_some());
    }

    #[test]
    fn compact_if_needed_swaps_entry_and_preserves_topology() {
        use fusiongraph_core::types::EdgeData;

        let catalog = GraphCatalog::new();
        catalog.register("g", test_graph()); // 4 base edges

        // Two delta mutations against a permissive policy: no compaction.
        let graph = catalog.get("g").unwrap();
        graph
            .delta()
            .insert(NodeId::new(3), NodeId::new(4), EdgeData::default());
        graph.delta().delete(NodeId::new(0), NodeId::new(1));

        let lax = CompactionPolicy {
            max_delta_entries: 100,
            max_delta_ratio: 10.0,
        };
        assert!(!catalog.compact_if_needed("g", &lax).unwrap());

        // Strict policy: compaction runs and the entry is swapped.
        let strict = CompactionPolicy {
            max_delta_entries: 1,
            max_delta_ratio: 10.0,
        };
        assert!(catalog.compact_if_needed("g", &strict).unwrap());

        let compacted = catalog.get("g").unwrap();
        assert!(compacted.delta().is_empty(), "delta merged into base");
        assert!(compacted.has_edge(NodeId::new(3), NodeId::new(4)));
        assert!(!compacted.has_edge(NodeId::new(0), NodeId::new(1)));

        // Unknown graphs are a no-op, not an error.
        assert!(!catalog.compact_if_needed("missing", &strict).unwrap());
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
