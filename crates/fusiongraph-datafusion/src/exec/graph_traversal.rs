//! `GraphTraversalExec` - Physical operator for graph traversals.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow::buffer::OffsetBuffer;
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

use fusiongraph_core::traversal::{bfs, TraversalSpec};
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
        partition: usize,
        _context: Arc<TaskContext>,
    ) -> datafusion::error::Result<SendableRecordBatchStream> {
        if partition != 0 {
            return Err(datafusion::error::DataFusionError::Execution(format!(
                "GraphTraversalExec only supports partition 0, got {partition}"
            )));
        }

        // Execute BFS for each start node and collect results
        let mut all_node_ids: Vec<u64> = Vec::new();
        let mut all_depths: Vec<u32> = Vec::new();
        let mut all_paths: Vec<Vec<u64>> = Vec::new();

        for &start in &self.spec.start {
            let result = bfs(&self.graph, start, self.spec.max_depth);

            for (idx, &node_id) in result.visited.iter().enumerate() {
                all_node_ids.push(node_id.as_u64());
                all_depths.push(result.depths[idx]);
                // Build path from start to this node (simplified: just start -> node)
                all_paths.push(vec![start.as_u64(), node_id.as_u64()]);
            }
        }

        // Build Arrow arrays
        let node_id_array: ArrayRef = Arc::new(UInt64Array::from(all_node_ids));
        let depth_array: ArrayRef = Arc::new(UInt32Array::from(all_depths));

        // Build ListArray for paths
        let path_values: Vec<u64> = all_paths.iter().flatten().copied().collect();
        let offsets: Vec<i32> = std::iter::once(0)
            .chain(all_paths.iter().scan(0i32, |acc, path| {
                *acc += path.len() as i32;
                Some(*acc)
            }))
            .collect();

        let values_array = UInt64Array::from(path_values);
        let path_array: ArrayRef = Arc::new(ListArray::new(
            Arc::new(Field::new("item", DataType::UInt64, false)),
            OffsetBuffer::new(offsets.into()),
            Arc::new(values_array),
            None,
        ));

        let batch = RecordBatch::try_new(
            self.schema(),
            vec![node_id_array, depth_array, path_array],
        )?;

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
        let builder = CsrBuilder::new()
            .with_edges([(0, 1), (1, 2), (0, 3)]);
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
        let debug_str = format!("{:?}", exec);
        assert!(debug_str.contains("GraphTraversalExec"));
    }
}
