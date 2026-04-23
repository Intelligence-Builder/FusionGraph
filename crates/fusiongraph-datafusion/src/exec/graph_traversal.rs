//! `GraphTraversalExec` - Physical operator for graph traversals.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow::array::{ArrayRef, ListArray, UInt32Array, UInt64Array};
use arrow::buffer::OffsetBuffer;
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::record_batch::RecordBatch;
use datafusion::execution::TaskContext;
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::memory::MemoryStream;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};

use fusiongraph_core::traversal::{bfs, bfs_multi, TraversalAlgorithm, TraversalSpec};
use fusiongraph_core::CsrGraph;

/// Physical operator for executing graph traversals.
#[derive(Debug)]
pub struct GraphTraversalExec {
    /// The graph to traverse.
    graph: Arc<CsrGraph>,
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
            spec,
            schema,
            properties,
        }
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
}

impl DisplayAs for GraphTraversalExec {
    fn fmt_as(&self, t: DisplayFormatType, f: &mut fmt::Formatter) -> fmt::Result {
        match t {
            DisplayFormatType::Default | DisplayFormatType::Verbose => {
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
        _partition: usize,
        _context: Arc<TaskContext>,
    ) -> datafusion::error::Result<SendableRecordBatchStream> {
        // Execute BFS traversal
        let result = match self.spec.algorithm {
            TraversalAlgorithm::Bfs => {
                if self.spec.start.len() == 1 {
                    bfs(&self.graph, self.spec.start[0], self.spec.max_depth)
                } else {
                    bfs_multi(&self.graph, &self.spec.start, self.spec.max_depth)
                }
            }
            TraversalAlgorithm::Dfs | TraversalAlgorithm::Dijkstra => {
                return Err(datafusion::error::DataFusionError::NotImplemented(
                    format!("{:?} traversal not yet implemented", self.spec.algorithm),
                ));
            }
        };

        // Convert BfsResult to RecordBatch
        let num_rows = result.visited.len();

        // node_id column
        let node_ids: UInt64Array = result.visited.iter().map(|n| n.as_u64()).collect();

        // depth column
        let depths: UInt32Array = result.depths.into_iter().collect();

        // path column (null for now - path tracking requires BFS modification)
        let path_field = Arc::new(Field::new("item", DataType::UInt64, false));
        let offsets = OffsetBuffer::from_lengths(std::iter::repeat(0).take(num_rows));
        let empty_values = UInt64Array::from(Vec::<u64>::new());
        let nulls = arrow::buffer::NullBuffer::new_null(num_rows);
        let path_array = ListArray::try_new(
            path_field,
            offsets,
            Arc::new(empty_values) as ArrayRef,
            Some(nulls),
        )
        .map_err(|e| datafusion::error::DataFusionError::ArrowError(e, None))?;

        let batch = RecordBatch::try_new(
            Arc::clone(&self.schema),
            vec![
                Arc::new(node_ids) as ArrayRef,
                Arc::new(depths) as ArrayRef,
                Arc::new(path_array) as ArrayRef,
            ],
        )
        .map_err(|e| datafusion::error::DataFusionError::ArrowError(e, None))?;

        Ok(Box::pin(MemoryStream::try_new(
            vec![batch],
            Arc::clone(&self.schema),
            None,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::physical_plan::ExecutionPlan;
    use fusiongraph_core::NodeId;
    use futures::StreamExt;

    fn make_test_graph() -> CsrGraph {
        // 0 → 1 → 3
        // ↓   ↓
        // 2 → 4
        CsrGraph::from_edges(&[(0, 1), (0, 2), (1, 3), (1, 4), (2, 4)])
    }

    #[test]
    fn output_schema_fields() {
        let schema = GraphTraversalExec::output_schema();
        assert_eq!(schema.fields().len(), 3);
        assert!(schema.field_with_name("node_id").is_ok());
        assert!(schema.field_with_name("depth").is_ok());
        assert!(schema.field_with_name("path").is_ok());
    }

    #[tokio::test]
    async fn execute_returns_traversal_results() {
        let graph = Arc::new(make_test_graph());
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 10,
            ..Default::default()
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = Arc::new(TaskContext::default());

        let mut stream = exec.execute(0, ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        assert_eq!(batch.num_rows(), 5); // All 5 nodes visited
        assert_eq!(batch.num_columns(), 3);

        let node_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(node_ids.value(0), 0); // Start node first
    }

    #[tokio::test]
    async fn execute_respects_max_depth() {
        let graph = Arc::new(make_test_graph());
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 1,
            ..Default::default()
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = Arc::new(TaskContext::default());

        let mut stream = exec.execute(0, ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        assert_eq!(batch.num_rows(), 3); // Only nodes 0, 1, 2 at depth <= 1
    }

    #[tokio::test]
    async fn execute_multi_start() {
        let graph = Arc::new(CsrGraph::from_edges(&[(0, 2), (1, 2), (2, 3)]));
        let spec = TraversalSpec {
            start: vec![NodeId::new(0), NodeId::new(1)],
            max_depth: 10,
            ..Default::default()
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = Arc::new(TaskContext::default());

        let mut stream = exec.execute(0, ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        assert_eq!(batch.num_rows(), 4); // All 4 nodes visited
    }

    #[tokio::test]
    async fn execute_empty_graph() {
        let graph = Arc::new(CsrGraph::empty());
        let spec = TraversalSpec {
            start: vec![NodeId::new(0)],
            max_depth: 10,
            ..Default::default()
        };

        let exec = GraphTraversalExec::new(graph, spec);
        let ctx = Arc::new(TaskContext::default());

        let mut stream = exec.execute(0, ctx).unwrap();
        let batch = stream.next().await.unwrap().unwrap();

        assert_eq!(batch.num_rows(), 0);
    }
}
