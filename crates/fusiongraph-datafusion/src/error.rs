//! DataFusion integration errors.

use thiserror::Error;

/// Errors from DataFusion integration.
#[derive(Error, Debug)]
pub enum DataFusionError {
    /// Graph error.
    #[error("Graph error: {0}")]
    Graph(#[from] fusiongraph_core::GraphError),

    /// Ontology error.
    #[error("Ontology error: {0}")]
    Ontology(#[from] fusiongraph_ontology::OntologyError),

    /// DataFusion error.
    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    /// Arrow error.
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Plan generation failed.
    #[error("FG-DFN-E001: Failed to generate execution plan: {reason}")]
    PlanGenerationFailed {
        /// Reason for failure.
        reason: String,
    },

    /// Operator execution failed.
    #[error("FG-DFN-E002: Execution failed in {operator}: {reason}")]
    ExecutionFailed {
        /// Operator name.
        operator: String,
        /// Reason for failure.
        reason: String,
    },
}

/// Result type alias.
pub type Result<T> = std::result::Result<T, DataFusionError>;
