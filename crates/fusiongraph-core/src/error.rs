//! Error types for FusionGraph core operations.

use thiserror::Error;

use crate::types::NodeId;

/// Errors that can occur during graph operations.
#[derive(Error, Debug)]
pub enum GraphError {
    /// Node was not found in the graph.
    #[error("FG-TRV-E001: Node {node_id} not found in graph")]
    NodeNotFound {
        /// The node ID that was not found.
        node_id: NodeId,
    },

    /// CSR build failed due to out of memory.
    #[error("FG-CSR-E001: Out of memory (requested {requested} bytes, available {available})")]
    OutOfMemory {
        /// Bytes requested.
        requested: usize,
        /// Bytes available.
        available: usize,
    },

    /// CSR shard corruption detected.
    #[error("FG-CSR-F001: Memory corruption in shard {shard_id}")]
    ShardCorruption {
        /// The corrupted shard ID.
        shard_id: u32,
    },

    /// Invalid edge data.
    #[error("FG-CSR-E002: Invalid edge: source {from} or target {to} out of range")]
    InvalidEdge {
        /// Source node ID.
        from: NodeId,
        /// Target node ID.
        to: NodeId,
    },

    /// Delta layer overflow.
    #[error("FG-DLT-E001: Delta layer overflow ({count} entries exceed threshold {threshold})")]
    DeltaOverflow {
        /// Current entry count.
        count: usize,
        /// Configured threshold.
        threshold: usize,
    },

    /// Traversal timeout.
    #[error("FG-TRV-E003: Traversal timed out after {duration_ms}ms (visited {nodes_visited} nodes)")]
    TraversalTimeout {
        /// Duration in milliseconds.
        duration_ms: u64,
        /// Number of nodes visited before timeout.
        nodes_visited: usize,
    },

    /// Invalid traversal specification.
    #[error("FG-TRV-E002: Invalid traversal: {reason}")]
    InvalidTraversal {
        /// Reason for invalidity.
        reason: String,
    },
}

impl GraphError {
    /// Returns the error code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NodeNotFound { .. } => "FG-TRV-E001",
            Self::OutOfMemory { .. } => "FG-CSR-E001",
            Self::ShardCorruption { .. } => "FG-CSR-F001",
            Self::InvalidEdge { .. } => "FG-CSR-E002",
            Self::DeltaOverflow { .. } => "FG-DLT-E001",
            Self::TraversalTimeout { .. } => "FG-TRV-E003",
            Self::InvalidTraversal { .. } => "FG-TRV-E002",
        }
    }

    /// Returns true if this is a fatal error.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::ShardCorruption { .. })
    }

    /// Returns true if the operation can be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::OutOfMemory { .. } | Self::TraversalTimeout { .. })
    }
}

/// Result type alias for graph operations.
pub type Result<T> = std::result::Result<T, GraphError>;
