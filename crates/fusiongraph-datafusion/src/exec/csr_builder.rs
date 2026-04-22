//! CSRBuilderExec - Physical operator for building CSR from Arrow streams.

use std::any::Any;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use arrow::array::{Array, ArrayRef, RecordBatch, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow_schema::SchemaRef;
use datafusion::error::DataFusionError;
use datafusion::execution::TaskContext;
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};
use futures::{Stream, StreamExt};

use fusiongraph_core::csr::CsrBuilder;

/// Configuration for CSR building.
#[derive(Debug, Clone)]
pub struct CsrBuildConfig {
    /// Shard size in bytes (default: 64MB).
    pub shard_size: usize,
    /// Parallelism for sorting/compacting.
    pub build_parallelism: usize,
    /// Memory limit for build operation.
    pub memory_limit: Option<usize>,
    /// Name of source column (default: "source").
    pub source_column: String,
    /// Name of target column (default: "target").
    pub target_column: String,
}

impl Default for CsrBuildConfig {
    fn default() -> Self {
        Self {
            shard_size: 64 * 1024 * 1024,
            build_parallelism: num_cpus::get(),
            memory_limit: None,
            source_column: "source".to_string(),
            target_column: "target".to_string(),
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
    /// Output schema (build statistics).
    schema: SchemaRef,
    /// Plan properties.
    properties: PlanProperties,
}

impl CSRBuilderExec {
    /// Creates a new CSRBuilderExec.
    pub fn new(input: Arc<dyn ExecutionPlan>, config: CsrBuildConfig) -> Self {
        let schema = Self::stats_schema();
        let properties = PlanProperties::new(
            EquivalenceProperties::new(Arc::clone(&schema)),
            Partitioning::UnknownPartitioning(1),
            EmissionType::Final,
            Boundedness::Bounded,
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

    /// Schema for build statistics output.
    fn stats_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("node_count", DataType::UInt64, false),
            Field::new("edge_count", DataType::UInt64, false),
            Field::new("shard_count", DataType::UInt64, false),
            Field::new("build_time_ms", DataType::UInt64, false),
        ]))
    }
}

impl DisplayAs for CSRBuilderExec {
    fn fmt_as(&self, t: DisplayFormatType, f: &mut fmt::Formatter) -> fmt::Result {
        match t {
            DisplayFormatType::Default | DisplayFormatType::Verbose => {
                write!(
                    f,
                    "CSRBuilderExec: shard_size={}, parallelism={}, source={}, target={}",
                    self.config.shard_size,
                    self.config.build_parallelism,
                    self.config.source_column,
                    self.config.target_column
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
        partition: usize,
        context: Arc<TaskContext>,
    ) -> datafusion::error::Result<SendableRecordBatchStream> {
        let input_stream = self.input.execute(partition, context)?;
        let config = self.config.clone();
        let schema = self.schema();
        let input_schema = self.input.schema();

        let stream = CsrBuildStream::new(input_stream, config, schema, input_schema);

        Ok(Box::pin(RecordBatchStreamAdapter::new(
            self.schema(),
            stream,
        )))
    }
}

/// Stream that collects edges and builds CSR.
struct CsrBuildStream {
    /// Input stream.
    input: SendableRecordBatchStream,
    /// Build configuration.
    config: CsrBuildConfig,
    /// Output schema.
    schema: SchemaRef,
    /// Input schema for column lookup.
    input_schema: SchemaRef,
    /// Collected edges.
    edges: Vec<(u64, u64)>,
    /// Whether we've finished building.
    finished: bool,
    /// Build result (if complete).
    result: Option<RecordBatch>,
}

impl CsrBuildStream {
    fn new(
        input: SendableRecordBatchStream,
        config: CsrBuildConfig,
        schema: SchemaRef,
        input_schema: SchemaRef,
    ) -> Self {
        Self {
            input,
            config,
            schema,
            input_schema,
            edges: Vec::new(),
            finished: false,
            result: None,
        }
    }

    fn extract_edges(&mut self, batch: &RecordBatch) -> Result<(), DataFusionError> {
        let source_idx = self
            .input_schema
            .index_of(&self.config.source_column)
            .map_err(|_| {
                DataFusionError::Execution(format!(
                    "Source column '{}' not found in input schema",
                    self.config.source_column
                ))
            })?;

        let target_idx = self
            .input_schema
            .index_of(&self.config.target_column)
            .map_err(|_| {
                DataFusionError::Execution(format!(
                    "Target column '{}' not found in input schema",
                    self.config.target_column
                ))
            })?;

        let sources = batch
            .column(source_idx)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| {
                DataFusionError::Execution(format!(
                    "Source column '{}' must be UInt64",
                    self.config.source_column
                ))
            })?;

        let targets = batch
            .column(target_idx)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| {
                DataFusionError::Execution(format!(
                    "Target column '{}' must be UInt64",
                    self.config.target_column
                ))
            })?;

        for i in 0..batch.num_rows() {
            if !sources.is_null(i) && !targets.is_null(i) {
                self.edges.push((sources.value(i), targets.value(i)));
            }
        }

        Ok(())
    }

    fn build_csr(&mut self) -> Result<RecordBatch, DataFusionError> {
        let start = std::time::Instant::now();

        let graph = CsrBuilder::new()
            .with_shard_size(self.config.shard_size)
            .with_edges(self.edges.drain(..))
            .build()
            .map_err(|e| DataFusionError::Execution(format!("CSR build failed: {}", e)))?;

        let elapsed_ms = start.elapsed().as_millis() as u64;

        let node_count = UInt64Array::from(vec![graph.node_count() as u64]);
        let edge_count = UInt64Array::from(vec![graph.edge_count() as u64]);
        let shard_count = UInt64Array::from(vec![graph.shard_count() as u64]);
        let build_time = UInt64Array::from(vec![elapsed_ms]);

        RecordBatch::try_new(
            Arc::clone(&self.schema),
            vec![
                Arc::new(node_count) as ArrayRef,
                Arc::new(edge_count) as ArrayRef,
                Arc::new(shard_count) as ArrayRef,
                Arc::new(build_time) as ArrayRef,
            ],
        )
        .map_err(|e| DataFusionError::ArrowError(e, None))
    }
}

impl Stream for CsrBuildStream {
    type Item = Result<RecordBatch, DataFusionError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        // Poll input stream for more batches
        loop {
            match self.input.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(batch))) => {
                    if let Err(e) = self.extract_edges(&batch) {
                        return Poll::Ready(Some(Err(e)));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    // Input exhausted, build CSR
                    match self.build_csr() {
                        Ok(batch) => {
                            self.finished = true;
                            return Poll::Ready(Some(Ok(batch)));
                        }
                        Err(e) => {
                            return Poll::Ready(Some(Err(e)));
                        }
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::UInt64Array;
    use datafusion::physical_plan::memory::MemoryExec;
    use datafusion::prelude::SessionContext;

    fn create_edge_batch(sources: Vec<u64>, targets: Vec<u64>) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("source", DataType::UInt64, false),
            Field::new("target", DataType::UInt64, false),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(sources)) as ArrayRef,
                Arc::new(UInt64Array::from(targets)) as ArrayRef,
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_csr_builder_exec() {
        let batch = create_edge_batch(vec![0, 0, 1, 2], vec![1, 2, 2, 3]);
        let schema = batch.schema();

        let input = Arc::new(MemoryExec::try_new(&[vec![batch]], schema, None).unwrap());
        let builder = CSRBuilderExec::new(input, CsrBuildConfig::default());

        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = builder.execute(0, task_ctx).unwrap();
        let result = stream.next().await.unwrap().unwrap();

        assert_eq!(result.num_rows(), 1);

        let node_count = result
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let edge_count = result
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        assert_eq!(node_count.value(0), 4); // nodes 0, 1, 2, 3
        assert_eq!(edge_count.value(0), 4); // 4 edges
    }

    #[tokio::test]
    async fn test_csr_builder_empty() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("source", DataType::UInt64, false),
            Field::new("target", DataType::UInt64, false),
        ]));

        let input = Arc::new(MemoryExec::try_new(&[vec![]], schema, None).unwrap());
        let builder = CSRBuilderExec::new(input, CsrBuildConfig::default());

        let ctx = SessionContext::new();
        let task_ctx = ctx.task_ctx();

        let mut stream = builder.execute(0, task_ctx).unwrap();
        let result = stream.next().await.unwrap().unwrap();

        let node_count = result
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(node_count.value(0), 0);
    }
}
