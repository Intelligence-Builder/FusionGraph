//! Direction-optimizing BFS (Beamer et al., SC'12).
//!
//! Level-synchronous BFS that switches between two expansion strategies:
//!
//! - **Top-down** (like [`bfs`](super::bfs())): scan the out-edges of the
//!   frontier. Cheap while the frontier is small.
//! - **Bottom-up**: scan the *in-edges of unvisited nodes* (via the
//!   transposed graph) and stop at the first parent found in the frontier.
//!   Dramatically cheaper when the frontier covers a large fraction of the
//!   graph, which is exactly what happens on skewed (R-MAT/social) graphs
//!   two or three hops from a hub.
//!
//! Switching heuristic (classic Graph500 parameters): go bottom-up when the
//! frontier's out-edge count exceeds `remaining unvisited edges / ALPHA`;
//! return to top-down when the frontier shrinks below `nodes / BETA`.
//!
//! Requires the transposed graph ([`CsrGraph::transpose`]) resident. Both
//! graphs must be delta-free: when the forward graph has live delta
//! mutations this function transparently falls back to the ordinary BFS
//! slow path, which applies them.
//!
//! Ordering note: visited order *within* a bottom-up level is ascending node
//! ID rather than discovery order. Level membership, depths, and counts are
//! identical to [`bfs`](super::bfs()).

use crate::csr::CsrGraph;
use crate::error::{GraphError, Result};
use crate::traversal::bfs::{bfs, BfsResult};
use crate::traversal::simd::select_backend;
use crate::types::NodeId;

/// Switch to bottom-up when `frontier_out_edges > unvisited_edges / ALPHA`.
const ALPHA: usize = 14;
/// Switch back to top-down when `frontier_len < node_count / BETA`.
const BETA: usize = 24;

#[inline]
fn test_bit(words: &[u64], i: u32) -> bool {
    (words[(i / 64) as usize] & (1u64 << (i % 64))) != 0
}

#[inline]
fn set_bit(words: &mut [u64], i: u32) {
    words[(i / 64) as usize] |= 1u64 << (i % 64);
}

/// Direction-optimizing BFS over `forward` using its transpose `reverse`.
///
/// # Errors
///
/// Returns [`GraphError::InvalidTraversal`] when `reverse` does not look
/// like the transpose of `forward` (node or edge counts differ) or when the
/// reverse graph carries delta mutations (the bottom-up phase reads base
/// slices only, so mutations on `reverse` would be silently ignored).
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn bfs_direction_optimized(
    forward: &CsrGraph,
    reverse: &CsrGraph,
    start: NodeId,
    max_depth: u32,
) -> Result<BfsResult> {
    if forward.node_count() != reverse.node_count() || forward.edge_count() != reverse.edge_count()
    {
        return Err(GraphError::InvalidTraversal {
            reason: format!(
                "reverse graph is not the transpose of forward: \
                 {}/{} nodes, {}/{} edges",
                forward.node_count(),
                reverse.node_count(),
                forward.edge_count(),
                reverse.edge_count()
            ),
        });
    }
    if !reverse.delta().is_empty() {
        return Err(GraphError::InvalidTraversal {
            reason: "reverse graph has delta mutations; rebuild the transpose".to_string(),
        });
    }

    // Live mutations on the forward graph require the merged iterator;
    // delegate to the ordinary BFS slow path for correctness.
    if !forward.delta().is_empty() {
        return Ok(bfs(forward, start, max_depth));
    }

    if !forward.contains(start) {
        return Ok(BfsResult {
            visited: Vec::new(),
            depths: Vec::new(),
            levels: Vec::new(),
            max_depth_reached: 0,
            edges_examined: 0,
        });
    }

    let node_count = forward.node_count();
    let words_len = node_count.div_ceil(64);
    let backend = select_backend();

    let mut visited_bits = vec![0u64; words_len];
    let mut frontier_bits = vec![0u64; words_len];

    let start_dense = start.as_u64() as u32;
    set_bit(&mut visited_bits, start_dense);

    let mut frontier: Vec<u32> = vec![start_dense];
    let mut next: Vec<u32> = Vec::new();
    let mut filtered: Vec<u32> = Vec::new();

    let mut visited: Vec<NodeId> = Vec::new();
    let mut depths: Vec<u32> = Vec::new();
    let mut levels: Vec<Vec<NodeId>> = Vec::new();
    let mut edges_examined = 0usize;
    // Out-edges already accounted to visited nodes (for the mu estimate).
    let mut visited_out_edges = 0usize;

    let degree = |n: u32| forward.base_neighbor_slice(NodeId::new(u64::from(n))).len();

    let mut depth = 0u32;
    loop {
        // Record the current level.
        let level: Vec<NodeId> = frontier
            .iter()
            .map(|&n| NodeId::new(u64::from(n)))
            .collect();
        for node in &level {
            visited.push(*node);
            depths.push(depth);
        }
        levels.push(level);

        if depth >= max_depth {
            break;
        }

        // Heuristic inputs.
        let frontier_out_edges: usize = frontier.iter().map(|&n| degree(n)).sum();
        visited_out_edges += frontier_out_edges;
        let unvisited_edges = forward.edge_count().saturating_sub(visited_out_edges);
        let bottom_up = frontier_out_edges > unvisited_edges / ALPHA
            && frontier.len() >= node_count / BETA.max(1);

        next.clear();
        if bottom_up {
            // Mark the frontier for O(1) parent membership tests.
            for &n in &frontier {
                set_bit(&mut frontier_bits, n);
            }

            // Scan in-edges of every unvisited node.
            for (word_idx, &word) in visited_bits.iter().enumerate() {
                let mut unvisited = !word;
                // Mask padding bits beyond node_count in the last word.
                if (word_idx + 1) * 64 > node_count {
                    let valid = node_count - word_idx * 64;
                    unvisited &= (1u64 << valid) - 1;
                }
                while unvisited != 0 {
                    let bit = unvisited.trailing_zeros();
                    unvisited &= unvisited - 1;
                    let u = (word_idx as u32) * 64 + bit;

                    let parents = reverse.base_neighbor_slice(NodeId::new(u64::from(u)));
                    for &p in parents {
                        edges_examined += 1;
                        if test_bit(&frontier_bits, p) {
                            next.push(u);
                            break;
                        }
                    }
                }
            }
            for &u in &next {
                set_bit(&mut visited_bits, u);
            }

            // Clear frontier bits for the next bottom-up level.
            for &n in &frontier {
                frontier_bits[(n / 64) as usize] &= !(1u64 << (n % 64));
            }
        } else {
            // Top-down: identical strategy to the ordinary fast path.
            for &n in &frontier {
                let neighbors = forward.base_neighbor_slice(NodeId::new(u64::from(n)));
                edges_examined += neighbors.len();
                backend.filter_unvisited_into(neighbors, &visited_bits, &mut filtered);
                for &m in &filtered {
                    if !test_bit(&visited_bits, m) {
                        set_bit(&mut visited_bits, m);
                        next.push(m);
                    }
                }
            }
        }

        if next.is_empty() {
            break;
        }
        std::mem::swap(&mut frontier, &mut next);
        depth += 1;
    }

    Ok(BfsResult {
        visited,
        depths,
        max_depth_reached: depth,
        edges_examined,
        levels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{rmat, uniform};
    use crate::types::EdgeData;
    use std::collections::HashSet;

    /// Levels must match `bfs` as *sets* (bottom-up levels are id-ordered).
    fn assert_equivalent(graph: &CsrGraph, start: NodeId, max_depth: u32) {
        let reverse = graph.transpose().unwrap();
        let reference = bfs(graph, start, max_depth);
        let hybrid = bfs_direction_optimized(graph, &reverse, start, max_depth).unwrap();

        assert_eq!(hybrid.node_count(), reference.node_count(), "node count");
        assert_eq!(
            hybrid.max_depth_reached, reference.max_depth_reached,
            "max depth"
        );
        assert_eq!(hybrid.levels.len(), reference.levels.len(), "level count");
        for (i, (h, r)) in hybrid.levels.iter().zip(&reference.levels).enumerate() {
            let h: HashSet<_> = h.iter().copied().collect();
            let r: HashSet<_> = r.iter().copied().collect();
            assert_eq!(h, r, "level {i} membership");
        }
        for node in &reference.visited {
            assert_eq!(
                hybrid.depth_of(*node),
                reference.depth_of(*node),
                "depth of {node:?}"
            );
        }
    }

    #[test]
    fn matches_bfs_on_small_topologies() {
        let chain = CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3)]);
        assert_equivalent(&chain, NodeId::new(0), 10);

        let star = CsrGraph::from_edges(&(1..64).map(|i| (0, i)).collect::<Vec<_>>());
        assert_equivalent(&star, NodeId::new(0), 10);

        let diamond = CsrGraph::from_edges(&[(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)]);
        assert_equivalent(&diamond, NodeId::new(0), 10);
    }

    #[test]
    fn matches_bfs_on_uniform_graph() {
        let graph = CsrGraph::from_edges(&uniform(2_000, 6, 0xACE));
        assert_equivalent(&graph, NodeId::new(0), 10);
    }

    #[test]
    fn matches_bfs_on_rmat_hub() {
        // Skewed graph traversed from the densest quadrant: exercises the
        // bottom-up switch.
        let graph = CsrGraph::from_edges(&rmat(12, 8, 0xACE));
        assert_equivalent(&graph, NodeId::new(0), 10);
    }

    #[test]
    fn respects_max_depth() {
        let graph = CsrGraph::from_edges(&uniform(500, 4, 7));
        assert_equivalent(&graph, NodeId::new(0), 2);
    }

    #[test]
    fn nonexistent_start_is_empty() {
        let graph = CsrGraph::from_edges(&[(0, 1)]);
        let reverse = graph.transpose().unwrap();
        let result = bfs_direction_optimized(&graph, &reverse, NodeId::new(99), 5).unwrap();
        assert_eq!(result.node_count(), 0);
    }

    #[test]
    fn forward_delta_falls_back_to_merged_bfs() {
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2)]);
        let reverse = graph.transpose().unwrap();
        graph
            .delta()
            .insert(NodeId::new(2), NodeId::new(3), EdgeData::default());

        // Falls back to the delta-aware slow path (reverse counts no longer
        // match is fine: fallback is checked after the shape validation, so
        // rebuild reverse first to pass it).
        let result = bfs_direction_optimized(&graph, &reverse, NodeId::new(0), 10).unwrap();
        assert!(result.contains(NodeId::new(3)), "delta insertion traversed");
    }

    #[test]
    fn mismatched_reverse_is_rejected() {
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2)]);
        let wrong = CsrGraph::from_edges(&[(0, 1)]);
        let err = bfs_direction_optimized(&graph, &wrong, NodeId::new(0), 5).unwrap_err();
        assert!(matches!(err, GraphError::InvalidTraversal { .. }));
        assert!(err.to_string().contains("FG-TRV-E002"));
    }

    #[test]
    fn reverse_with_delta_is_rejected() {
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2)]);
        let reverse = graph.transpose().unwrap();
        reverse
            .delta()
            .insert(NodeId::new(5), NodeId::new(6), EdgeData::default());
        let err = bfs_direction_optimized(&graph, &reverse, NodeId::new(0), 5).unwrap_err();
        assert!(matches!(err, GraphError::InvalidTraversal { .. }));
    }
}
