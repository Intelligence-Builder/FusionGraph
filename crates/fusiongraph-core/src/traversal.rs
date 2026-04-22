//! Graph traversal algorithms.
//!
//! Provides BFS, DFS, and shortest-path algorithms over CSR graphs.

mod bfs;

pub use bfs::{bfs, BfsResult};

use crate::types::NodeId;

/// Result of a graph traversal.
#[derive(Debug, Clone)]
pub struct TraversalResult {
    /// Visited nodes in traversal order.
    pub visited: Vec<NodeId>,
    /// Depth of each visited node (parallel to `visited`).
    pub depths: Vec<u32>,
    /// Maximum depth reached during traversal.
    pub max_depth_reached: u32,
    /// Number of edges traversed.
    pub edges_traversed: usize,
}

impl TraversalResult {
    /// Creates an empty traversal result.
    pub fn empty() -> Self {
        Self {
            visited: Vec::new(),
            depths: Vec::new(),
            max_depth_reached: 0,
            edges_traversed: 0,
        }
    }

    /// Returns the number of visited nodes.
    pub fn node_count(&self) -> usize {
        self.visited.len()
    }

    /// Returns nodes at a specific depth.
    pub fn nodes_at_depth(&self, depth: u32) -> Vec<NodeId> {
        self.visited
            .iter()
            .zip(&self.depths)
            .filter_map(|(&node, &d)| if d == depth { Some(node) } else { None })
            .collect()
    }
}

/// Specification for a traversal operation.
#[derive(Debug, Clone)]
pub struct TraversalSpec {
    /// Starting node(s).
    pub start: Vec<NodeId>,
    /// Maximum depth to traverse.
    pub max_depth: u32,
    /// Maximum number of nodes to visit.
    pub max_nodes: Option<usize>,
    /// Traversal algorithm.
    pub algorithm: TraversalAlgorithm,
    /// Direction to traverse.
    pub direction: TraversalDirection,
}

impl Default for TraversalSpec {
    fn default() -> Self {
        Self {
            start: Vec::new(),
            max_depth: 10,
            max_nodes: None,
            algorithm: TraversalAlgorithm::Bfs,
            direction: TraversalDirection::Outgoing,
        }
    }
}

/// Traversal algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalAlgorithm {
    /// Breadth-first search.
    Bfs,
    /// Depth-first search.
    Dfs,
    /// Dijkstra's shortest path (requires weighted graph).
    Dijkstra,
}

/// Direction of traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow outgoing edges.
    Outgoing,
    /// Follow incoming edges.
    Incoming,
    /// Follow both directions.
    Both,
}
