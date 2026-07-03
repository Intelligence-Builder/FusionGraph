//! Breadth-First Search implementation.
//!
//! ## Hot path (M4)
//!
//! When the delta layer is empty, BFS runs an allocation-free kernel:
//! zero-copy `&[u32]` neighbor slices from CSR storage are batch-filtered
//! against a dense `u64` visited bitset through the platform
//! [`SimdBackend`](super::simd::SimdBackend) (NEON on `aarch64`, AVX2 on
//! `x86_64`, scalar elsewhere). When delta mutations exist, BFS falls back
//! to the merged [`CsrGraph::neighbors`] iterator, which applies insertions
//! and tombstones; visited tracking still uses the dense bitset, with a
//! hash-set overflow for delta nodes beyond the base ID range.

use std::collections::{HashSet, VecDeque};

use crate::csr::CsrGraph;
use crate::traversal::simd::{select_backend, SimdBackend};
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
    fn empty() -> Self {
        Self {
            visited: Vec::new(),
            depths: Vec::new(),
            levels: Vec::new(),
            max_depth_reached: 0,
            edges_examined: 0,
        }
    }

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

/// Dense bitset visited tracker with a hash-set overflow for node IDs beyond
/// the base graph's dense range (possible via delta-layer insertions).
struct Visited {
    words: Vec<u64>,
    capacity: u64,
    overflow: HashSet<NodeId>,
}

impl Visited {
    fn new(node_count: usize) -> Self {
        Self {
            words: vec![0u64; node_count.div_ceil(64)],
            capacity: node_count as u64,
            overflow: HashSet::new(),
        }
    }

    /// Marks `node` visited; returns `true` if it was previously unvisited.
    #[inline]
    fn test_and_set(&mut self, node: NodeId) -> bool {
        let id = node.as_u64();
        if id < self.capacity {
            #[allow(clippy::cast_possible_truncation)]
            let (word, bit) = ((id / 64) as usize, id % 64);
            let mask = 1u64 << bit;
            let was_unset = self.words[word] & mask == 0;
            self.words[word] |= mask;
            was_unset
        } else {
            self.overflow.insert(node)
        }
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
    bfs_bounded(graph, &[start], max_depth, None)
}

/// Performs BFS from multiple starting nodes simultaneously.
pub fn bfs_multi(graph: &CsrGraph, starts: &[NodeId], max_depth: u32) -> BfsResult {
    bfs_bounded(graph, starts, max_depth, None)
}

/// Performs BFS from multiple starting nodes with an optional cap on the
/// number of visited nodes.
///
/// This is the traversal kernel used by higher layers (e.g. the `DataFusion`
/// `GraphTraversalExec` operator). `max_nodes` bounds the total number of
/// nodes admitted to the traversal (start nodes included).
pub fn bfs_bounded(
    graph: &CsrGraph,
    starts: &[NodeId],
    max_depth: u32,
    max_nodes: Option<usize>,
) -> BfsResult {
    bfs_bounded_with_backend(
        graph,
        starts,
        max_depth,
        max_nodes,
        select_backend().as_ref(),
    )
}

/// [`bfs_bounded`] with an explicit SIMD backend (primarily for benchmarking
/// backend implementations against each other).
#[allow(clippy::too_many_lines)]
pub fn bfs_bounded_with_backend(
    graph: &CsrGraph,
    starts: &[NodeId],
    max_depth: u32,
    max_nodes: Option<usize>,
    backend: &dyn SimdBackend,
) -> BfsResult {
    let max_nodes = max_nodes.unwrap_or(usize::MAX);
    let mut visited_tracker = Visited::new(graph.node_count());
    let mut admitted = 0usize;

    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();
    for &start in starts {
        if admitted >= max_nodes {
            break;
        }
        if graph.contains(start) && visited_tracker.test_and_set(start) {
            queue.push_back((start, 0));
            admitted += 1;
        }
    }
    if queue.is_empty() {
        return BfsResult::empty();
    }

    let mut visited: Vec<NodeId> = Vec::new();
    let mut depths: Vec<u32> = Vec::new();
    let mut levels: Vec<Vec<NodeId>> = Vec::new();
    let mut edges_examined = 0usize;
    let mut current_depth = 0;
    let mut current_level: Vec<NodeId> = Vec::new();

    // Fast path applies when no delta mutations exist: base slices are the
    // complete, tombstone-free adjacency.
    let delta_empty = graph.delta().is_empty();
    // Reusable batch buffer for SIMD filtering (allocation-free steady state).
    let mut filtered: Vec<u32> = Vec::new();

    while let Some((node, depth)) = queue.pop_front() {
        if depth > current_depth {
            levels.push(std::mem::take(&mut current_level));
            current_depth = depth;
        }

        visited.push(node);
        depths.push(depth);
        current_level.push(node);

        if depth >= max_depth || admitted >= max_nodes {
            continue;
        }

        if delta_empty {
            // Zero-copy neighbor slice, batch-filtered via SIMD backend.
            let neighbors = graph.base_neighbor_slice(node);
            edges_examined += neighbors.len();
            backend.filter_unvisited_into(neighbors, &visited_tracker.words, &mut filtered);
            for &n in &filtered {
                if admitted >= max_nodes {
                    break;
                }
                let neighbor = NodeId::new(u64::from(n));
                // Re-check under mark: handles duplicates within the batch.
                if visited_tracker.test_and_set(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                    admitted += 1;
                }
            }
        } else {
            // Slow path: merged base + delta iterator (applies tombstones).
            for neighbor in graph.neighbors(node) {
                edges_examined += 1;
                if admitted >= max_nodes {
                    break;
                }
                if visited_tracker.test_and_set(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                    admitted += 1;
                }
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
    use crate::traversal::simd::ScalarBackend;
    use crate::types::EdgeData;

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
        assert_eq!(result.depth_of(NodeId(2)), Some(1));
    }

    #[test]
    fn bfs_bounded_caps_visited_nodes() {
        let graph = make_test_graph();
        let result = bfs_bounded(&graph, &[NodeId(0)], 10, Some(2));
        assert_eq!(result.node_count(), 2);
    }

    #[test]
    fn bfs_slow_path_applies_delta_insertions_and_tombstones() {
        let graph = make_test_graph();
        // Insert 4 -> 5 (new node beyond base range) and delete 0 -> 2.
        graph
            .delta()
            .insert(NodeId(4), NodeId(5), EdgeData::default());
        graph.delta().delete(NodeId(0), NodeId(2));

        let result = bfs(&graph, NodeId(0), 10);
        assert!(result.contains(NodeId(5)), "delta insertion traversed");
        // 2 is still reachable at depth 1? No: 0 -> 2 deleted, and no other
        // in-edges to 2 exist, so 2 must be unreachable.
        assert!(!result.contains(NodeId(2)), "tombstoned edge not traversed");
        assert_eq!(result.depth_of(NodeId(5)), Some(3));
    }

    #[test]
    fn fast_path_matches_slow_path_semantics() {
        // Same topology built twice: once pure-base (fast path), once with
        // the last edge applied via the delta layer (slow path).
        let base_edges = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 4)];
        let fast = CsrGraph::from_edges(&base_edges);

        let slow = CsrGraph::from_edges(&base_edges[..4]);
        slow.delta()
            .insert(NodeId(2), NodeId(4), EdgeData::default());

        let fast_result = bfs(&fast, NodeId(0), 10);
        let slow_result = bfs(&slow, NodeId(0), 10);

        assert_eq!(fast_result.node_count(), slow_result.node_count());
        for node in &fast_result.visited {
            assert_eq!(
                fast_result.depth_of(*node),
                slow_result.depth_of(*node),
                "depth mismatch for {node:?}"
            );
        }
    }

    #[test]
    fn explicit_backend_matches_default() {
        let graph = make_test_graph();
        let default_result = bfs(&graph, NodeId(0), 10);
        let scalar_result =
            bfs_bounded_with_backend(&graph, &[NodeId(0)], 10, None, &ScalarBackend);

        assert_eq!(default_result.visited, scalar_result.visited);
        assert_eq!(default_result.depths, scalar_result.depths);
        assert_eq!(default_result.edges_examined, scalar_result.edges_examined);
    }
}
