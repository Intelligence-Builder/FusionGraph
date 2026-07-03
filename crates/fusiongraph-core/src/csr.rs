//! Compressed Sparse Row (CSR) graph storage with micro-sharding.
//!
//! The CSR format stores graphs in contiguous memory for cache-efficient traversal.
//! Micro-sharding partitions the graph into 64MB chunks to prevent compaction walls.

#![allow(
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

mod builder;
mod shard;

pub use builder::CsrBuilder;
pub use shard::CsrShard;

use std::collections::HashSet;
use std::sync::Arc;

use crate::delta::DeltaLayer;
use crate::types::{GraphStatistics, NodeId};

/// Default shard size: 64MB.
pub const DEFAULT_SHARD_SIZE: usize = 64 * 1024 * 1024;

/// A CSR-based graph with micro-sharding support.
#[derive(Debug)]
pub struct CsrGraph {
    /// Shards containing the base layer topology.
    shards: Vec<Arc<CsrShard>>,
    /// Total node count across all shards.
    node_count: usize,
    /// Total edge count across all shards.
    edge_count: usize,
    /// Delta layer for real-time updates.
    delta: Arc<DeltaLayer>,
}

impl CsrGraph {
    fn node_index(node: NodeId) -> Option<usize> {
        usize::try_from(node.as_u64()).ok()
    }

    /// Creates an empty graph.
    pub fn empty() -> Self {
        Self {
            shards: Vec::new(),
            node_count: 0,
            edge_count: 0,
            delta: Arc::new(DeltaLayer::new()),
        }
    }

    /// Creates a graph from a list of edges.
    ///
    /// This is primarily for testing. For production use, use [`CsrBuilder`].
    ///
    /// # Panics
    ///
    /// Panics if `edges` contains values that violate the invariants enforced
    /// by [`CsrBuilder::build`].
    pub fn from_edges(edges: &[(u64, u64)]) -> Self {
        CsrBuilder::new()
            .with_edges(edges.iter().copied())
            .build()
            .expect("building from edges should not fail")
    }

    /// Returns the total number of nodes.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Returns the total number of edges (base layer only).
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    /// Returns the number of shards.
    #[inline]
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Returns a reference to the shards.
    #[cfg(test)]
    #[inline]
    pub(crate) fn shards(&self) -> &[Arc<CsrShard>] {
        &self.shards
    }

    /// Returns true if the graph contains the given node.
    #[inline]
    pub fn contains(&self, node: NodeId) -> bool {
        Self::node_index(node).is_some_and(|id| id < self.node_count)
    }

    /// Returns the shard containing the given node.
    pub fn shard_for(&self, node: NodeId) -> Option<&Arc<CsrShard>> {
        let (shard_idx, _) = self.global_to_shard(node)?;
        self.shards.get(shard_idx)
    }

    /// Converts a global `NodeId` to (`shard_index`, `local_offset`).
    #[inline]
    pub fn global_to_shard(&self, node: NodeId) -> Option<(usize, usize)> {
        let id = Self::node_index(node)?;
        let shard_idx = self
            .shards
            .partition_point(|shard| shard.node_range().start <= id)
            .checked_sub(1)?;
        let shard = self.shards.get(shard_idx)?;

        if shard.contains(id) {
            Some((shard_idx, id - shard.node_range().start))
        } else {
            None
        }
    }

    /// Converts (`shard_index`, `local_offset`) to a global `NodeId`.
    #[inline]
    pub fn shard_to_global(&self, shard_idx: usize, offset: usize) -> Option<NodeId> {
        self.shards.get(shard_idx).and_then(|shard| {
            if offset >= shard.node_count() {
                return None;
            }
            let global = shard.node_range().start + offset;
            u64::try_from(global).ok().map(NodeId::new)
        })
    }

    /// Returns the out-degree of a node (number of outgoing edges).
    pub fn out_degree(&self, node: NodeId) -> usize {
        self.neighbors(node).count()
    }

    /// Returns an iterator over the neighbors of a node.
    pub fn neighbors(&self, node: NodeId) -> NeighborIter<'_> {
        NeighborIter {
            graph: self,
            node,
            base_iter: self.base_neighbors(node),
            delta_iter: self.delta.neighbors(node),
            base_seen: HashSet::new(),
        }
    }

    /// Returns the base-layer neighbors of a node as a contiguous slice of
    /// dense `u32` IDs (zero-copy view into CSR storage).
    ///
    /// Excludes delta-layer insertions and does **not** apply delta
    /// tombstones; callers on the traversal fast path must check
    /// [`DeltaLayer::is_empty`] first (see [`Self::delta`]).
    #[inline]
    #[must_use]
    pub fn base_neighbor_slice(&self, node: NodeId) -> &[u32] {
        self.global_to_shard(node)
            .and_then(|(shard_idx, offset)| {
                self.shards
                    .get(shard_idx)
                    .map(|shard| shard.neighbor_slice(offset))
            })
            .unwrap_or(&[])
    }

    /// Returns an iterator over base layer neighbors only.
    fn base_neighbors(&self, node: NodeId) -> BaseNeighborIter<'_> {
        self.global_to_shard(node)
            .and_then(|(shard_idx, offset)| {
                self.shards
                    .get(shard_idx)
                    .map(|shard| BaseNeighborIter::new(shard, offset))
            })
            .unwrap_or_else(BaseNeighborIter::empty)
    }

    /// Checks if an edge exists between two nodes.
    pub fn has_edge(&self, from: NodeId, to: NodeId) -> bool {
        // Check delta deletions first
        if self.delta.is_deleted(from, to) {
            return false;
        }

        // Check delta insertions
        if self.delta.has_insertion(from, to) {
            return true;
        }

        // Check base layer
        self.neighbors(from).any(|n| n == to)
    }

    /// Returns a reference to the delta layer.
    pub fn delta(&self) -> &Arc<DeltaLayer> {
        &self.delta
    }

    /// Returns memory usage statistics.
    pub fn memory_usage(&self) -> usize {
        let shard_memory: usize = self.shards.iter().map(|s| s.memory_usage()).sum();
        let delta_memory = self.delta.memory_usage();
        shard_memory + delta_memory
    }

    /// Compacts the delta layer into a new base-only graph.
    ///
    /// LSM-style merge: delta insertions are materialized into CSR storage,
    /// tombstoned base edges are dropped, and the returned graph has an
    /// empty delta layer — restoring the allocation-free traversal fast
    /// path (the delta slow path measures ~74x slower; see benches).
    ///
    /// **Drains this graph's delta layer** (standard LSM compaction
    /// semantics): after the call, `self` observes the same topology minus
    /// the drained mutations, so callers should replace `self` with the
    /// returned graph.
    ///
    /// Edge weights: base-layer weights are preserved; weights attached to
    /// delta insertions via [`crate::types::EdgeData`] are carried over
    /// (`f64` truncated to the CSR's `f32` storage, defaulting to 1.0).
    ///
    /// # Errors
    ///
    /// Returns an error if the merged graph exceeds CSR capacity limits
    /// (see [`CsrBuilder::build`]).
    pub fn compact(&self) -> crate::error::Result<Self> {
        let deletions: HashSet<(NodeId, NodeId)> =
            self.delta.drain_deletions().into_iter().collect();
        let insertions = self.delta.drain_insertions();

        let has_weights = self.shards.iter().any(|s| s.has_weights());

        let mut edges: Vec<(u64, u64)> = Vec::with_capacity(self.edge_count + insertions.len());
        let mut weights: Vec<f32> = if has_weights {
            Vec::with_capacity(self.edge_count + insertions.len())
        } else {
            Vec::new()
        };

        // Base edges, minus tombstones.
        for shard in &self.shards {
            for local in 0..shard.node_count() {
                let Some(from) = self.shard_to_global(shard.id() as usize, local) else {
                    continue;
                };
                let (start, end) = shard.neighbor_range(local);
                for idx in start..end {
                    let Some(to) = shard.col_index(idx) else {
                        continue;
                    };
                    let to = NodeId::from(to);
                    if deletions.contains(&(from, to)) {
                        continue;
                    }
                    edges.push((from.as_u64(), to.as_u64()));
                    if has_weights {
                        weights.push(shard.weight(idx).unwrap_or(1.0));
                    }
                }
            }
        }

        // Delta insertions (disjoint from tombstones by DeltaLayer
        // invariants). Skip pairs already present in base: NeighborIter
        // yields such pairs once, and compaction preserves those semantics.
        for ((from, to), data) in insertions {
            if self.base_contains(from, to) {
                continue;
            }
            edges.push((from.as_u64(), to.as_u64()));
            if has_weights {
                #[allow(clippy::cast_possible_truncation)]
                weights.push(data.weight.map_or(1.0, |w| w as f32));
            }
        }

        if has_weights {
            CsrBuilder::new()
                .with_weighted_edges(
                    edges
                        .iter()
                        .copied()
                        .zip(weights.iter().copied())
                        .map(|((f, t), w)| (f, t, w)),
                )
                .build()
        } else {
            CsrBuilder::new().with_edges(edges).build()
        }
    }

    /// Builds the transposed graph: every edge `(a, b)` becomes `(b, a)`.
    ///
    /// The transpose enables incoming-edge traversal ("who can reach X?")
    /// as a plain outgoing BFS on the reversed topology — the standard
    /// pattern for blast-radius-*inverse* queries — and is the building
    /// block for future direction-optimizing BFS (ROADMAP M4).
    ///
    /// The snapshot includes live delta mutations (insertions reversed,
    /// tombstoned base edges dropped); the returned graph has an empty
    /// delta layer and does **not** track subsequent mutations of `self`.
    ///
    /// # Errors
    ///
    /// Returns an error if the transposed graph exceeds CSR capacity limits
    /// (see [`CsrBuilder::build`]).
    pub fn transpose(&self) -> crate::error::Result<Self> {
        let deletions: HashSet<(NodeId, NodeId)> =
            self.delta.snapshot_deletions().into_iter().collect();

        let mut edges: Vec<(u64, u64)> = Vec::with_capacity(self.edge_count);

        // Base edges (tombstones dropped), reversed.
        for shard in &self.shards {
            for local in 0..shard.node_count() {
                let Some(from) = self.shard_to_global(shard.id() as usize, local) else {
                    continue;
                };
                let (start, end) = shard.neighbor_range(local);
                for idx in start..end {
                    let Some(to) = shard.col_index(idx) else {
                        continue;
                    };
                    let to = NodeId::from(to);
                    if deletions.contains(&(from, to)) {
                        continue;
                    }
                    edges.push((to.as_u64(), from.as_u64()));
                }
            }
        }

        // Delta insertions reversed — including edges whose source node is
        // beyond the base range. Base-duplicate pairs are skipped to match
        // NeighborIter's dedupe semantics.
        for ((from, to), _) in self.delta.snapshot_insertions() {
            if self.base_contains(from, to) {
                continue;
            }
            edges.push((to.as_u64(), from.as_u64()));
        }

        CsrBuilder::new().with_edges(edges).build()
    }

    /// Returns whether the base layer contains the edge `(from, to)`
    /// (ignores the delta layer).
    fn base_contains(&self, from: NodeId, to: NodeId) -> bool {
        u32::try_from(to.as_u64()).is_ok_and(|t| self.base_neighbor_slice(from).contains(&t))
    }

    /// Returns comprehensive statistics about the graph.
    pub fn statistics(&self) -> GraphStatistics {
        GraphStatistics {
            node_count: self.node_count,
            edge_count: self.edge_count,
            shard_count: self.shards.len(),
            memory_bytes: self.memory_usage(),
            delta_entries: self.delta.len(),
        }
    }
}

/// Iterator over neighbors of a node in the base layer.
#[derive(Debug)]
pub struct BaseNeighborIter<'a> {
    shard: Option<&'a CsrShard>,
    current: usize,
    end: usize,
}

impl<'a> BaseNeighborIter<'a> {
    fn new(shard: &'a CsrShard, node_offset: usize) -> Self {
        let (start, end) = shard.neighbor_range(node_offset);
        Self {
            shard: Some(shard),
            current: start,
            end,
        }
    }

    fn empty() -> Self {
        Self {
            shard: None,
            current: 0,
            end: 0,
        }
    }
}

impl Iterator for BaseNeighborIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.end {
            let shard = self.shard?;
            let neighbor = shard.col_index(self.current)?;
            self.current += 1;
            Some(NodeId::from(neighbor))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end.saturating_sub(self.current);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for BaseNeighborIter<'_> {}

/// Iterator over all neighbors of a node (base + delta, excluding tombstones).
pub struct NeighborIter<'a> {
    graph: &'a CsrGraph,
    node: NodeId,
    base_iter: BaseNeighborIter<'a>,
    delta_iter: std::vec::IntoIter<NodeId>,
    base_seen: HashSet<NodeId>,
}

impl Iterator for NeighborIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        // First, yield base layer neighbors (skip tombstoned)
        for neighbor in self.base_iter.by_ref() {
            if !self.graph.delta.is_deleted(self.node, neighbor) {
                self.base_seen.insert(neighbor);
                return Some(neighbor);
            }
        }

        // Then yield delta insertions
        self.delta_iter
            .by_ref()
            .find(|neighbor| !self.base_seen.contains(neighbor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph() {
        let graph = CsrGraph::empty();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert!(!graph.contains(NodeId::new(0)));
    }

    #[test]
    fn from_edges() {
        let graph = CsrGraph::from_edges(&[(0, 1), (0, 2), (1, 2)]);
        assert!(graph.node_count() >= 3);
        assert_eq!(graph.edge_count(), 3);
        assert!(graph.contains(NodeId::new(0)));
        assert!(graph.has_edge(NodeId::new(0), NodeId::new(1)));
        assert!(graph.has_edge(NodeId::new(0), NodeId::new(2)));
        assert!(!graph.has_edge(NodeId::new(2), NodeId::new(0)));
    }

    #[test]
    fn neighbors_iteration() {
        let graph = CsrGraph::from_edges(&[(0, 1), (0, 2), (0, 3)]);
        assert_eq!(graph.neighbors(NodeId::new(0)).count(), 3);
    }

    #[test]
    fn shard_indexing_roundtrip() {
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3)]);
        let node = NodeId::new(1);
        if let Some((shard_idx, offset)) = graph.global_to_shard(node) {
            let recovered = graph.shard_to_global(shard_idx, offset);
            assert_eq!(recovered, Some(node));
        }
    }

    #[test]
    fn neighbors_deduplicate_delta_edges_against_base() {
        let graph = CsrGraph::from_edges(&[(0, 1), (0, 2)]);
        graph.delta().insert(
            NodeId::new(0),
            NodeId::new(2),
            crate::types::EdgeData::default(),
        );
        graph.delta().insert(
            NodeId::new(0),
            NodeId::new(3),
            crate::types::EdgeData::default(),
        );

        let neighbors: Vec<_> = graph.neighbors(NodeId::new(0)).collect();

        assert_eq!(
            neighbors,
            vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
        );
    }

    #[test]
    fn out_degree_ignores_tombstones_for_non_base_edges() {
        let graph = CsrGraph::from_edges(&[(0, 1), (0, 2)]);
        graph.delta().delete(NodeId::new(0), NodeId::new(99));

        assert_eq!(graph.out_degree(NodeId::new(0)), 2);
    }

    #[test]
    fn out_degree_does_not_double_count_delta_duplicates() {
        let graph = CsrGraph::from_edges(&[(0, 1)]);
        graph.delta().insert(
            NodeId::new(0),
            NodeId::new(1),
            crate::types::EdgeData::default(),
        );

        assert_eq!(graph.out_degree(NodeId::new(0)), 1);
    }

    // =========================================================================
    // Compaction
    // =========================================================================

    fn dirty_graph() -> CsrGraph {
        // Base: 0 -> 1 -> 2 -> 3. Delta: insert 3 -> 4, delete 1 -> 2.
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3)]);
        graph.delta().insert(
            NodeId::new(3),
            NodeId::new(4),
            crate::types::EdgeData::default(),
        );
        graph.delta().delete(NodeId::new(1), NodeId::new(2));
        graph
    }

    #[test]
    fn compact_materializes_delta_and_empties_it() {
        let graph = dirty_graph();
        let compacted = graph.compact().unwrap();

        assert!(compacted.delta().is_empty(), "new graph has empty delta");
        assert!(compacted.has_edge(NodeId::new(3), NodeId::new(4)));
        assert!(!compacted.has_edge(NodeId::new(1), NodeId::new(2)));
        assert_eq!(compacted.edge_count(), 3); // 0->1, 2->3, 3->4
    }

    #[test]
    fn compact_preserves_traversal_semantics() {
        let graph = dirty_graph();
        let before = crate::traversal::bfs(&graph, NodeId::new(0), 10);
        let compacted = graph.compact().unwrap();
        let after = crate::traversal::bfs(&compacted, NodeId::new(0), 10);

        assert_eq!(before.visited, after.visited);
        assert_eq!(before.depths, after.depths);
    }

    #[test]
    fn compact_skips_delta_duplicates_of_base_edges() {
        let graph = CsrGraph::from_edges(&[(0, 1)]);
        graph.delta().insert(
            NodeId::new(0),
            NodeId::new(1),
            crate::types::EdgeData::default(),
        );

        let compacted = graph.compact().unwrap();
        assert_eq!(compacted.edge_count(), 1, "no parallel edge introduced");
    }

    #[test]
    fn compact_preserves_weights() {
        let graph = CsrBuilder::new()
            .with_weighted_edges([(0u64, 1u64, 2.5f32), (1, 2, 0.5)])
            .build()
            .unwrap();
        graph.delta().insert(
            NodeId::new(2),
            NodeId::new(3),
            crate::types::EdgeData {
                weight: Some(9.0),
                label: None,
            },
        );

        let compacted = graph.compact().unwrap();
        assert_eq!(compacted.edge_count(), 3);
        // Weight of the base edge 0 -> 1 survives compaction.
        let (shard_idx, offset) = compacted.global_to_shard(NodeId::new(0)).unwrap();
        let shard = &compacted.shards()[shard_idx];
        let (start, _) = shard.neighbor_range(offset);
        assert_eq!(shard.weight(start), Some(2.5));
    }

    // =========================================================================
    // Transpose
    // =========================================================================

    #[test]
    fn transpose_reverses_edges() {
        let graph = CsrGraph::from_edges(&[(0, 1), (0, 2), (1, 2)]);
        let rev = graph.transpose().unwrap();

        assert_eq!(rev.edge_count(), 3);
        assert!(rev.has_edge(NodeId::new(1), NodeId::new(0)));
        assert!(rev.has_edge(NodeId::new(2), NodeId::new(0)));
        assert!(rev.has_edge(NodeId::new(2), NodeId::new(1)));
        assert!(!rev.has_edge(NodeId::new(0), NodeId::new(1)));
    }

    #[test]
    fn transpose_applies_delta_mutations() {
        let graph = dirty_graph(); // base 0->1->2->3, +3->4, -1->2
        let rev = graph.transpose().unwrap();

        assert!(rev.has_edge(NodeId::new(4), NodeId::new(3)), "insertion");
        assert!(!rev.has_edge(NodeId::new(2), NodeId::new(1)), "tombstone");
        assert!(rev.has_edge(NodeId::new(1), NodeId::new(0)));
    }

    #[test]
    fn transpose_enables_incoming_traversal() {
        // Who can reach node 3? In 0 -> 1 -> 2 -> 3: everyone.
        let graph = CsrGraph::from_edges(&[(0, 1), (1, 2), (2, 3)]);
        let rev = graph.transpose().unwrap();
        let result = crate::traversal::bfs(&rev, NodeId::new(3), 10);

        assert_eq!(result.node_count(), 4);
        assert_eq!(result.depth_of(NodeId::new(0)), Some(3));
    }

    #[test]
    fn double_transpose_is_identity_topology() {
        let edges = [(0u64, 1u64), (0, 2), (1, 3), (2, 3), (3, 4)];
        let graph = CsrGraph::from_edges(&edges);
        let round_trip = graph.transpose().unwrap().transpose().unwrap();

        assert_eq!(round_trip.edge_count(), edges.len());
        for &(f, t) in &edges {
            assert!(round_trip.has_edge(NodeId::new(f), NodeId::new(t)));
        }
    }
}
