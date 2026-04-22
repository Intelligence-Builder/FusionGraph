//! Breadth-First Search implementation.

use std::collections::{HashSet, VecDeque};

use crate::csr::CsrGraph;
use crate::types::NodeId;

/// Result of a BFS traversal.
#[derive(Debug, Clone)]
pub struct BfsResult {
    /// Visited nodes in BFS order.
    pub visited: Vec<NodeId>,
    /// Depth of each visited node.
    pub depths: Vec<u32>,
    /// Nodes at each level (level -> nodes).
    pub levels: Vec<Vec<NodeId>>,
    /// Maximum depth reached.
    pub max_depth_reached: u32,
    /// Number of edges examined.
    pub edges_examined: usize,
}

impl BfsResult {
    /// Returns the number of visited nodes.
    pub fn node_count(&self) -> usize {
        self.visited.len()
    }

    /// Returns true if the given node was visited.
    pub fn contains(&self, node: NodeId) -> bool {
        self.visited.contains(&node)
    }

    /// Returns the depth of a node, if visited.
    pub fn depth_of(&self, node: NodeId) -> Option<u32> {
        self.visited
            .iter()
            .position(|&n| n == node)
            .map(|idx| self.depths[idx])
    }
}

/// Performs breadth-first search from a starting node.
///
/// # Arguments
/// * `graph` - The graph to traverse
/// * `start` - Starting node
/// * `max_depth` - Maximum depth to traverse (0 = start node only)
///
/// # Returns
/// A `BfsResult` containing visited nodes and their depths.
pub fn bfs(graph: &CsrGraph, start: NodeId, max_depth: u32) -> BfsResult {
    if !graph.contains(start) {
        return BfsResult {
            visited: Vec::new(),
            depths: Vec::new(),
            levels: Vec::new(),
            max_depth_reached: 0,
            edges_examined: 0,
        };
    }

    let mut visited_set: HashSet<NodeId> = HashSet::new();
    let mut visited: Vec<NodeId> = Vec::new();
    let mut depths: Vec<u32> = Vec::new();
    let mut levels: Vec<Vec<NodeId>> = Vec::new();
    let mut edges_examined = 0usize;

    // BFS queue: (node, depth)
    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

    // Initialize with start node
    queue.push_back((start, 0));
    visited_set.insert(start);

    let mut current_depth = 0;
    let mut current_level: Vec<NodeId> = Vec::new();

    while let Some((node, depth)) = queue.pop_front() {
        // Track level changes
        if depth > current_depth {
            levels.push(std::mem::take(&mut current_level));
            current_depth = depth;
        }

        visited.push(node);
        depths.push(depth);
        current_level.push(node);

        // Stop expanding if at max depth
        if depth >= max_depth {
            continue;
        }

        // Explore neighbors
        for neighbor in graph.neighbors(node) {
            edges_examined += 1;

            if !visited_set.contains(&neighbor) {
                visited_set.insert(neighbor);
                queue.push_back((neighbor, depth + 1));
            }
        }
    }

    // Push final level
    if !current_level.is_empty() {
        levels.push(current_level);
    }

    BfsResult {
        visited,
        depths,
        levels,
        max_depth_reached: current_depth,
        edges_examined,
    }
}

/// Performs BFS from multiple starting nodes simultaneously.
#[allow(dead_code)]
pub fn bfs_multi(graph: &CsrGraph, starts: &[NodeId], max_depth: u32) -> BfsResult {
    let mut visited_set: HashSet<NodeId> = HashSet::new();
    let mut visited: Vec<NodeId> = Vec::new();
    let mut depths: Vec<u32> = Vec::new();
    let mut levels: Vec<Vec<NodeId>> = Vec::new();
    let mut edges_examined = 0usize;

    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

    // Initialize with all start nodes at depth 0
    for &start in starts {
        if graph.contains(start) && !visited_set.contains(&start) {
            queue.push_back((start, 0));
            visited_set.insert(start);
        }
    }

    let mut current_depth = 0;
    let mut current_level: Vec<NodeId> = Vec::new();

    while let Some((node, depth)) = queue.pop_front() {
        if depth > current_depth {
            levels.push(std::mem::take(&mut current_level));
            current_depth = depth;
        }

        visited.push(node);
        depths.push(depth);
        current_level.push(node);

        if depth >= max_depth {
            continue;
        }

        for neighbor in graph.neighbors(node) {
            edges_examined += 1;

            if !visited_set.contains(&neighbor) {
                visited_set.insert(neighbor);
                queue.push_back((neighbor, depth + 1));
            }
        }
    }

    if !current_level.is_empty() {
        levels.push(current_level);
    }

    BfsResult {
        visited,
        depths,
        levels,
        max_depth_reached: current_depth,
        edges_examined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_graph() -> CsrGraph {
        // Graph:
        //     0 → 1 → 3
        //     ↓   ↓
        //     2 → 4
        CsrGraph::from_edges(&[(0, 1), (0, 2), (1, 3), (1, 4), (2, 4)])
    }

    #[test]
    fn bfs_from_root() {
        let graph = make_test_graph();
        let result = bfs(&graph, NodeId(0), 10);

        assert_eq!(result.node_count(), 5);
        assert!(result.contains(NodeId(0)));
        assert!(result.contains(NodeId(4)));

        // Check depths
        assert_eq!(result.depth_of(NodeId(0)), Some(0));
        assert_eq!(result.depth_of(NodeId(1)), Some(1));
        assert_eq!(result.depth_of(NodeId(2)), Some(1));
        assert_eq!(result.depth_of(NodeId(3)), Some(2));
        assert_eq!(result.depth_of(NodeId(4)), Some(2));
    }

    #[test]
    fn bfs_respects_max_depth() {
        let graph = make_test_graph();
        let result = bfs(&graph, NodeId(0), 1);

        assert_eq!(result.node_count(), 3); // 0, 1, 2
        assert!(result.contains(NodeId(0)));
        assert!(result.contains(NodeId(1)));
        assert!(result.contains(NodeId(2)));
        assert!(!result.contains(NodeId(3)));
        assert!(!result.contains(NodeId(4)));
    }

    #[test]
    fn bfs_depth_zero() {
        let graph = make_test_graph();
        let result = bfs(&graph, NodeId(0), 0);

        assert_eq!(result.node_count(), 1);
        assert!(result.contains(NodeId(0)));
    }

    #[test]
    fn bfs_nonexistent_start() {
        let graph = make_test_graph();
        let result = bfs(&graph, NodeId(999), 10);

        assert_eq!(result.node_count(), 0);
    }

    #[test]
    fn bfs_levels() {
        let graph = make_test_graph();
        let result = bfs(&graph, NodeId(0), 10);

        assert_eq!(result.levels.len(), 3);
        assert_eq!(result.levels[0], vec![NodeId(0)]);
        assert!(result.levels[1].contains(&NodeId(1)));
        assert!(result.levels[1].contains(&NodeId(2)));
    }

    #[test]
    fn bfs_multi_starts() {
        let graph = CsrGraph::from_edges(&[(0, 2), (1, 2), (2, 3)]);
        let result = bfs_multi(&graph, &[NodeId(0), NodeId(1)], 10);

        assert!(result.contains(NodeId(0)));
        assert!(result.contains(NodeId(1)));
        assert!(result.contains(NodeId(2)));
        assert!(result.contains(NodeId(3)));

        // Both 0 and 1 should be at depth 0
        assert_eq!(result.depth_of(NodeId(0)), Some(0));
        assert_eq!(result.depth_of(NodeId(1)), Some(0));
    }
}
