//! Error types for FusionGraph core operations.
//!
//! Error codes follow the format: `FG-{SUBSYSTEM}-{SEVERITY}{NUMBER}`
//! - Subsystems: ONT, CSR, DLT, TRV, FFI, MEM
//! - Severity: F (Fatal), E (Error), W (Warning)

use thiserror::Error;

use crate::types::NodeId;

/// Severity level for errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Unrecoverable state corruption - requires shutdown.
    Fatal,
    /// Operation failed - request rejected.
    Error,
    /// Degraded but functional - possible performance impact.
    Warning,
}

impl Severity {
    /// Returns the severity code character.
    pub fn code(&self) -> char {
        match self {
            Self::Fatal => 'F',
            Self::Error => 'E',
            Self::Warning => 'W',
        }
    }

    /// Returns a human-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// Errors that can occur during graph operations.
#[derive(Error, Debug)]
pub enum GraphError {
    // =========================================================================
    // CSR Errors (CSR)
    // =========================================================================
    /// CSR build failed due to out of memory.
    #[error("FG-CSR-E001: Out of memory (requested {requested} bytes, available {available})")]
    OutOfMemory {
        /// Bytes requested.
        requested: usize,
        /// Bytes available.
        available: usize,
    },

    /// Invalid edge data.
    #[error("FG-CSR-E002: Invalid edge: source {from} or target {to} out of range")]
    InvalidEdge {
        /// Source node ID.
        from: NodeId,
        /// Target node ID.
        to: NodeId,
    },

    /// Graph exceeds the CSR representation limits.
    #[error("FG-CSR-E003: Unsupported graph size: {reason}")]
    UnsupportedGraphSize {
        /// Human-readable size or capacity failure.
        reason: String,
    },

    /// CSR shard corruption detected.
    #[error("FG-CSR-F001: Memory corruption in shard {shard_id} (checksum {expected:x} != {actual:x})")]
    ShardCorruption {
        /// The corrupted shard ID.
        shard_id: u32,
        /// Expected checksum.
        expected: u64,
        /// Actual checksum.
        actual: u64,
    },

    // =========================================================================
    // Delta Layer Errors (DLT)
    // =========================================================================
    /// Delta layer overflow.
    #[error("FG-DLT-E001: Delta layer overflow ({count} entries exceed threshold {threshold})")]
    DeltaOverflow {
        /// Current entry count.
        count: usize,
        /// Configured threshold.
        threshold: usize,
    },

    /// Delta compaction failed.
    #[error("FG-DLT-E002: Delta compaction failed: {reason}")]
    CompactionFailed {
        /// Reason for failure.
        reason: String,
    },

    // =========================================================================
    // Traversal Errors (TRV)
    // =========================================================================
    /// Node was not found in the graph.
    #[error("FG-TRV-E001: Node {node_id} not found in graph")]
    NodeNotFound {
        /// The node ID that was not found.
        node_id: NodeId,
    },

    /// Invalid traversal specification.
    #[error("FG-TRV-E002: Invalid traversal: {reason}")]
    InvalidTraversal {
        /// Reason for invalidity.
        reason: String,
    },

    /// Traversal timeout.
    #[error(
        "FG-TRV-E003: Traversal timed out after {duration_ms}ms (visited {nodes_visited} nodes)"
    )]
    TraversalTimeout {
        /// Duration in milliseconds.
        duration_ms: u64,
        /// Number of nodes visited before timeout.
        nodes_visited: usize,
    },

    /// Cycle limit exceeded during traversal.
    #[error("FG-TRV-E004: Cycle limit exceeded at node {node_id} (visited {visit_count} times)")]
    CycleLimitExceeded {
        /// Node where cycle was detected.
        node_id: NodeId,
        /// Number of times the node was visited.
        visit_count: usize,
    },

    // =========================================================================
    // Memory Errors (MEM)
    // =========================================================================
    /// Memory limit exceeded.
    #[error("FG-MEM-E001: Memory limit exceeded (limit {limit} bytes, requested {requested})")]
    MemoryLimitExceeded {
        /// Configured memory limit.
        limit: usize,
        /// Requested allocation.
        requested: usize,
    },

    // =========================================================================
    // System Errors (SYS)
    // =========================================================================
    /// Circuit breaker is open.
    #[error("FG-SYS-E001: Circuit breaker open, failing fast")]
    CircuitOpen,

    /// Internal error (unexpected state).
    #[error("FG-SYS-E002: Internal error: {message}")]
    Internal {
        /// Error message.
        message: String,
    },
}

impl GraphError {
    /// Returns the error code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutOfMemory { .. } => "FG-CSR-E001",
            Self::InvalidEdge { .. } => "FG-CSR-E002",
            Self::UnsupportedGraphSize { .. } => "FG-CSR-E003",
            Self::ShardCorruption { .. } => "FG-CSR-F001",
            Self::DeltaOverflow { .. } => "FG-DLT-E001",
            Self::CompactionFailed { .. } => "FG-DLT-E002",
            Self::NodeNotFound { .. } => "FG-TRV-E001",
            Self::InvalidTraversal { .. } => "FG-TRV-E002",
            Self::TraversalTimeout { .. } => "FG-TRV-E003",
            Self::CycleLimitExceeded { .. } => "FG-TRV-E004",
            Self::MemoryLimitExceeded { .. } => "FG-MEM-E001",
            Self::CircuitOpen => "FG-SYS-E001",
            Self::Internal { .. } => "FG-SYS-E002",
        }
    }

    /// Returns the subsystem code.
    pub fn subsystem(&self) -> &'static str {
        match self {
            Self::OutOfMemory { .. }
            | Self::InvalidEdge { .. }
            | Self::UnsupportedGraphSize { .. }
            | Self::ShardCorruption { .. } => "CSR",
            Self::DeltaOverflow { .. } | Self::CompactionFailed { .. } => "DLT",
            Self::NodeNotFound { .. }
            | Self::InvalidTraversal { .. }
            | Self::TraversalTimeout { .. }
            | Self::CycleLimitExceeded { .. } => "TRV",
            Self::MemoryLimitExceeded { .. } => "MEM",
            Self::CircuitOpen | Self::Internal { .. } => "SYS",
        }
    }

    /// Returns the severity of this error.
    pub fn severity(&self) -> Severity {
        match self {
            Self::ShardCorruption { .. } => Severity::Fatal,
            _ => Severity::Error,
        }
    }

    /// Returns true if this is a fatal error.
    pub fn is_fatal(&self) -> bool {
        self.severity() == Severity::Fatal
    }

    /// Returns true if the operation can be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::OutOfMemory { .. }
                | Self::TraversalTimeout { .. }
                | Self::CircuitOpen
                | Self::MemoryLimitExceeded { .. }
        )
    }
}

/// Result type alias for graph operations.
pub type Result<T> = std::result::Result<T, GraphError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_match_format() {
        let errors = [
            GraphError::OutOfMemory {
                requested: 100,
                available: 50,
            },
            GraphError::NodeNotFound {
                node_id: NodeId::new(1),
            },
            GraphError::ShardCorruption {
                shard_id: 0,
                expected: 123,
                actual: 456,
            },
        ];

        for err in errors {
            let code = err.code();
            assert!(code.starts_with("FG-"), "Code should start with FG-: {code}");
            assert!(code.len() >= 10, "Code should be at least 10 chars: {code}");
        }
    }

    #[test]
    fn fatal_errors_identified() {
        let fatal = GraphError::ShardCorruption {
            shard_id: 0,
            expected: 0,
            actual: 1,
        };
        assert!(fatal.is_fatal());
        assert_eq!(fatal.severity(), Severity::Fatal);

        let non_fatal = GraphError::NodeNotFound {
            node_id: NodeId::new(1),
        };
        assert!(!non_fatal.is_fatal());
    }

    #[test]
    fn retryable_errors_identified() {
        let retryable = GraphError::OutOfMemory {
            requested: 100,
            available: 50,
        };
        assert!(retryable.is_retryable());

        let not_retryable = GraphError::InvalidEdge {
            from: NodeId::new(0),
            to: NodeId::new(1),
        };
        assert!(!not_retryable.is_retryable());
    }

    #[test]
    fn subsystems_correct() {
        assert_eq!(
            GraphError::OutOfMemory {
                requested: 0,
                available: 0
            }
            .subsystem(),
            "CSR"
        );
        assert_eq!(
            GraphError::DeltaOverflow {
                count: 0,
                threshold: 0
            }
            .subsystem(),
            "DLT"
        );
        assert_eq!(
            GraphError::NodeNotFound {
                node_id: NodeId::new(0)
            }
            .subsystem(),
            "TRV"
        );
    }
}
