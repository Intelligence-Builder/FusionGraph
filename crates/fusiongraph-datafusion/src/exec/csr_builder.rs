//! CSRBuilderExec - Physical operator for building CSR from Arrow streams.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::execution::TaskContext;
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};

use fusiongraph_core::CsrGraph;

/// Configuration for CSR building.
#[derive(Debug, Clone)]
pub struct CsrBuildConfig {
    /// Shard size in bytes (default: 64MB).
    pub shard_size: usize,
    /// Parallelism for sorting/compacting.
    pub build_parallelism: usize,
    /// Memory limit for build operation.
    pub memory_limit: Option<usize>,
}

impl Default for CsrBuildConfig {
    fn default() -> Self {
        Self {
            shard_size: 64 * 1024 * 1024,
            build_parallelism: num_cpus::get(),
            memory_limit: None,
        }
    }
}

/// Physical operator that builds a CSR graph from Arrow RecordBatch streams.
#[derive(Debug)]
pub struct CSRBuilderExec {
    /// Input execution plan.
    input: Arc<dyn ExecutionPlan>,
    /// Build configuration.
    config: CsrBuildConfig,
    /// Output schema.
    schema: SchemaRef,
    /// Plan properties.
    properties: PlanProperties,
}

impl CSRBuilderExec {
    /// Creates a new CSRBuilderExec.
    pub fn new(input: Arc<dyn ExecutionPlan>, config: CsrBuildConfig) -> Self {
        let schema = input.schema();
        let properties = PlanProperties::new(
            EquivalenceProperties::new(Arc::clone(&schema)),
            Partitioning::UnknownPartitioning(1),
            datafusion::physical_plan::ExecutionMode::Bounded,
        );

        Self {
            input,
            config,
            schema,
            properties,
        }
    }

    /// Returns the build configuration.
    pub fn config(&self) -> &CsrBuildConfig {
        &self.config
    }
}

impl DisplayAs for CSRBuilderExec {
    fn fmt_as(&self, t: DisplayFormatType, f: &mut fmt::Formatter) -> fmt::Result {
        match t {
            DisplayFormatType::Default | DisplayFormatType::Verbose => {
                write!(
                    f,
                    "CSRBuilderExec: shard_size={}, parallelism={}",
                    self.config.shard_size, self.config.build_parallelism
                )
            }
        }
    }
}

impl ExecutionPlan for CSRBuilderExec {
    fn name(&self) -> &str {
        "CSRBuilderExec"
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
        vec![&self.input]
    }

    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> datafusion::error::Result<Arc<dyn ExecutionPlan>> {
        Ok(Arc::new(Self::new(
            Arc::clone(&children[0]),
            self.config.clone(),
        )))
    }

    fn execute(
        &self,
        _partition: usize,
        _context: Arc<TaskContext>,
    ) -> datafusion::error::Result<SendableRecordBatchStream> {
        // TODO: Implement CSR building from RecordBatch stream
        Err(datafusion::error::DataFusionError::NotImplemented(
            "CSRBuilderExec::execute not yet implemented".to_string(),
        ))
    }
}
