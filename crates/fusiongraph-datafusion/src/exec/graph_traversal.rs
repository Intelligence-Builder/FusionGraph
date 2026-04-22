//! `GraphTraversalExec` - Physical operator for graph traversals.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use datafusion::execution::TaskContext;
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};

use fusiongraph_core::traversal::TraversalSpec;
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
        // TODO: Implement traversal execution
        Err(datafusion::error::DataFusionError::NotImplemented(
            "GraphTraversalExec::execute not yet implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_schema_fields() {
        let schema = GraphTraversalExec::output_schema();
        assert_eq!(schema.fields().len(), 3);
        assert!(schema.field_with_name("node_id").is_ok());
        assert!(schema.field_with_name("depth").is_ok());
    }
}
