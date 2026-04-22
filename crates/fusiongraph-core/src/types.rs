//! Core type definitions for FusionGraph.

use std::fmt;

/// A unique identifier for a node in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

impl NodeId {
    /// Creates a new NodeId from a u64 value.
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw u64 value.
    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

impl From<u64> for NodeId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        Self(u64::from(id))
    }
}

/// A unique identifier for an edge in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u64);

impl EdgeId {
    /// Creates a new EdgeId from a u64 value.
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw u64 value.
    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Edge({})", self.0)
    }
}

/// Edge data stored in the delta layer.
#[derive(Debug, Clone, Default)]
pub struct EdgeData {
    /// Optional edge weight.
    pub weight: Option<f64>,
    /// Optional edge label.
    pub label: Option<String>,
}

/// Statistics about the graph.
#[derive(Debug, Clone, Default)]
pub struct GraphStatistics {
    /// Total number of nodes.
    pub node_count: usize,
    /// Total number of edges.
    pub edge_count: usize,
    /// Number of shards.
    pub shard_count: usize,
    /// Memory usage in bytes.
    pub memory_bytes: usize,
    /// Delta layer entry count.
    pub delta_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_roundtrip() {
        let id = NodeId::new(42);
        assert_eq!(id.as_u64(), 42);
        assert_eq!(NodeId::from(42u64), id);
    }

    #[test]
    fn node_id_display() {
        let id = NodeId::new(123);
        assert_eq!(format!("{id}"), "Node(123)");
    }
}
