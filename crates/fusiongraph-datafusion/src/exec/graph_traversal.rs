//! `GraphTraversalExec` - Physical operator for graph traversals.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow::buffer::{NullBuffer, OffsetBuffer};
use arrow_array::{ArrayRef, ListArray, RecordBatch, UInt32Array, UInt64Array};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use datafusion::execution::TaskContext;
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};
use futures::stream;

use fusiongraph_core::traversal::{TraversalAlgorithm, TraversalDirection, TraversalSpec};
use fusiongraph_core::CsrGraph;

/// Physical operator for executing graph traversals.
#[derive(Debug)]
pub struct GraphTraversalExec {
    /// The graph to traverse.
    graph: Arc<CsrGraph>,
    /// Optional transposed graph ([`CsrGraph::transpose`]). Enables
    /// direction-optimizing BFS for outgoing traversals and
    /// [`TraversalDirection::Incoming`] ("who can reach X?").
    reverse: Option<Arc<CsrGraph>>,
    /// Traversal specification.
    spec: TraversalSpec,
    /// Output schema.
    schema: SchemaRef,
    /// Plan properties.
    properties: PlanProperties,
}

impl GraphTraversalExec {
    /// Creates a new `GraphTraversalExec`.
    #[must_use]
    pub fn new(graph: Arc<CsrGraph>, spec: TraversalSpec) -> Self {
        let schema = Self::output_schema();
        let properties = PlanProperties::new(
            EquivalenceProperties::new(Arc::clone(&schema)),
            Partitioning::UnknownPartitioning(1),
            EmissionType::Final,
            Boundedness::Bounded,
        );

        Self {
            graph,
            reverse: None,
            spec,
            schema,
            properties,
        }
    }

    /// Attaches the transposed graph.
    ///
    /// With a reverse graph resident, outgoing traversals from a single
    /// start node use direction-optimizing BFS (2.2–3.2x on skewed hub
    /// traversals), and [`TraversalDirection::Incoming`] becomes
    /// executable as an outgoing traversal of the reverse topology.
    #[must_use]
    pub fn with_reverse(mut self, reverse: Arc<CsrGraph>) -> Self {
        self.reverse = Some(reverse);
        self
    }

    /// Returns the output schema for traversal results.
    fn output_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("node_id", DataType::UInt64, false),
            Field::new("depth", DataType::UInt32, false),
            Field::new(
                "path",
                DataType::List(Arc::new(Field::new("item", DataType::UInt64, false))),
                true,
            ),
        ]))
    }

    /// Returns the traversal specification.
    #[must_use]
    pub const fn spec(&self) -> &TraversalSpec {
        &self.spec
    }

    /// Returns a reference to the graph.
    #[must_use]
    pub const fn graph(&self) -> &Arc<CsrGraph> {
        &self.graph
    }

    fn validate_spec(&self) -> datafusion::error::Result<()> {
        if self.spec.algorithm != TraversalAlgorithm::Bfs {
            return Err(datafusion::error::DataFusionError::NotImplemented(format!(
                "GraphTraversalExec only supports BFS traversal, got {:?}",
                self.spec.algorithm
            )));
        }

        match self.spec.direction {
            TraversalDirection::Outgoing => Ok(()),
            TraversalDirection::Incoming => {
                if self.reverse.is_some() {
                    Ok(())
                } else {
                    Err(datafusion::error::DataFusionError::Plan(
                        "incoming traversal requires the transposed graph; attach it \
                         with GraphTraversalExec::with_reverse (built via \
                         CsrGraph::transpose) or register the graph with a reverse \
                         in the GraphCatalog"
                            .to_string(),
                    ))
                }
            }
            TraversalDirection::Both => Err(datafusion::error::DataFusionError::NotImplemented(
                "GraphTraversalExec does not support bidirectional traversal yet".to_string(),
            )),
        }
    }

    /// Runs the traversal through the core BFS kernel (SIMD fast path when
    /// the delta layer is empty) and returns Arrow-ready column vectors.
    fn collect_bfs_rows(&self, max_nodes: usize) -> (Vec<u64>, Vec<u32>) {
        if max_nodes == 0 {
            return (Vec::new(), Vec::new());
        }

        let bounded = (max_nodes != usize::MAX).then_some(max_nodes);

        // Incoming traversal = outgoing traversal of the transpose
        // (validate_spec guarantees `reverse` is present for Incoming).
        let (forward, reverse) = match self.spec.direction {
            TraversalDirection::Incoming => {
                (self.reverse.as_ref().expect("validated"), Some(&self.graph))
            }
            _ => (&self.graph, self.reverse.as_ref()),
        };

        // Direction-optimizing BFS applies to single-start, uncapped
        // traversals with a resident transpose; anything else (or a
        // shape/delta mismatch) falls back to the plain kernel.
        let result = match (reverse, bounded, self.spec.start.as_slice()) {
            (Some(rev), None, [start]) => fusiongraph_core::traversal::bfs_direction_optimized(
                forward,
                rev,
                *start,
                self.spec.max_depth,
            )
            .unwrap_or_else(|_| {
                fusiongraph_core::traversal::bfs_bounded(
                    forward,
                    &self.spec.start,
                    self.spec.max_depth,
                    bounded,
                )
            }),
            _ => fusiongraph_core::traversal::bfs_bounded(
                forward,
                &self.spec.start,
                self.spec.max_depth,
                bounded,
            ),
        };

        let node_ids = result.visited.iter().map(|n| n.as_u64()).collect();
        (node_ids, result.depths)
    }
}

impl DisplayAs for GraphTraversalExec {
    fn fmt_as(&self, t: DisplayFormatType, f: &mut fmt::Formatter) -> fmt::Result {
        match t {
            DisplayFormatType::Default
            | DisplayFormatType::Verbose
            | DisplayFormatType::TreeRender => {
                write!(
                    f,
                    "GraphTraversalExec: algorithm={:?}, max_depth={}, starts={}",
                    self.spec.algorithm,
                    self.spec.max_depth,
                    self.spec.start.len()
                )
            }
        }
    }
}

impl ExecutionPlan for GraphTraversalExec {
    fn name(&self) -> &'static str {
        "GraphTraversalExec"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    fn properties(&self) -> &PlanProperties {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        vec![]
    }

    fn with_new_children(
        self: Arc<Self>,
        _children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> datafusion::error::Result<Arc<dyn ExecutionPlan>> {
        Ok(self)
    }

    fn execute(
        &self,
        partition: usize,
        _context: Arc<TaskContext>,
    ) -> datafusion::error::Result<SendableRecordBatchStream> {
        if partition != 0 {
            return Err(datafusion::error::DataFusionError::Execution(format!(
                "GraphTraversalExec only supports partition 0, got {partition}"
            )));
        }

        self.validate_spec()?;

        let max_nodes = self.spec.max_nodes.unwrap_or(usize::MAX);
        let (all_node_ids, all_depths) = self.collect_bfs_rows(max_nodes);
        let row_count = all_node_ids.len();

        // Build Arrow arrays
        let node_id_array: ArrayRef = Arc::new(UInt64Array::from(all_node_ids));
        let depth_array: ArrayRef = Arc::new(UInt32Array::from(all_depths));

        // Parent tracking is not implemented yet, so expose the nullable path
        // column as NULL instead of returning misleading synthetic paths.
        let offsets = vec![0_i32; row_count + 1];
        let values_array = UInt64Array::from(Vec::<u64>::new());
        let path_array: ArrayRef = Arc::new(ListArray::new(
            Arc::new(Field::new("item", DataType::UInt64, false)),
            OffsetBuffer::new(offsets.into()),
            Arc::new(values_array),
            Some((0..row_count).map(|_| false).collect::<NullBuffer>()),
        ));

        let batch =
            RecordBatch::try_new(self.schema(), vec![node_id_array, depth_array, path_array])?;

        let schema = self.schema();
        let stream = stream::once(async move { Ok(batch) });

        Ok(Box::pin(RecordBatchStreamAdapter::new(schema, stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;
    use datafusion::prelude::SessionContext;
    use fusiongraph_core::csr::CsrBuilder;
    use fusiongraph_core::traversal::{TraversalAlgorithm, TraversalDirection};
    use fusiongraph_core::types::NodeId;
    use futures::StreamExt;

    #[test]
    fn output_schema_fields() {
        let schema = GraphTraversalExec::output_schema();
        assert_eq!(schema.fields().len(), 3);
        assert!(schema.field_with_name("node_id").is_ok());
        assert!(schema.field_with_name("depth").is_ok());
    }

    fn build_test_graph() -> Arc<CsrGraph> {
        // Build a simple graph: 0 -> 1 -> 2, 0 -> 3
        let builder = CsrBuilder::new().with_edges([(0, 1), (1, 2), (0, 3)]);
        Arc::new(builder.build().unwrap())
    }

    #[tokio::test]
    async fn execute_returns_traversal_results() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 2,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = exec.execute(0, task_ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        // Should have visited nodes: 0, 1, 3, 2
        assert!(batch.num_rows() >= 3);

        let node_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert!(node_ids.iter().any(|v| v == Some(0)));
        assert!(node_ids.iter().any(|v| v == Some(1)));
    }

    #[tokio::test]
    async fn execute_respects_max_depth() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 1,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = exec.execute(0, task_ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        let depths = batch
            .column(1)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();

        // All depths should be <= 1
        for depth in depths.iter().flatten() {
            assert!(depth <= 1);
        }
    }

    #[tokio::test]
    async fn execute_rejects_non_bfs_algorithm() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 2,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Dfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let Err(err) = exec.execute(0, task_ctx) else {
            panic!("DFS traversal should not be accepted before implementation");
        };
        assert!(err.to_string().contains("only supports BFS traversal"));
    }

    #[tokio::test]
    async fn execute_rejects_non_outgoing_direction() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 2,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Incoming,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let Err(err) = exec.execute(0, task_ctx) else {
            panic!("incoming traversal should not be accepted before implementation");
        };
        assert!(err
            .to_string()
            .contains("incoming traversal requires the transposed graph"));
    }

    #[tokio::test]
    async fn execute_enforces_max_nodes() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 3,
            max_nodes: Some(2),
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = exec.execute(0, task_ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        assert_eq!(batch.num_rows(), 2);
    }

    #[tokio::test]
    async fn execute_returns_null_paths_until_parent_tracking_exists() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 2,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = exec.execute(0, task_ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();
        let paths = batch
            .column(2)
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();

        assert_eq!(paths.len(), batch.num_rows());
        assert_eq!(paths.null_count(), batch.num_rows());
    }

    #[test]
    fn display_format() {
        let graph = build_test_graph();
        let spec = TraversalSpec {
            start: vec![NodeId::new(0), NodeId::new(1)],
            max_depth: 3,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        };

        let exec = GraphTraversalExec::new(graph, spec);
        // Verify DisplayAs is implemented - use Debug format for testing
        let debug_str = format!("{exec:?}");
        assert!(debug_str.contains("GraphTraversalExec"));
    }
}
