//! Compressed Sparse Row (CSR) graph storage with micro-sharding.
//!
//! The CSR format stores graphs in contiguous memory for cache-efficient traversal.
//! Micro-sharding partitions the graph into 64MB chunks to prevent compaction walls.

mod builder;
mod shard;

pub use builder::CsrBuilder;
pub use shard::CsrShard;

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

    /// Returns true if the graph contains the given node.
    #[inline]
    pub fn contains(&self, node: NodeId) -> bool {
        (node.as_u64() as usize) < self.node_count
    }

    /// Returns the shard containing the given node.
    pub fn shard_for(&self, node: NodeId) -> Option<&Arc<CsrShard>> {
        let (shard_idx, _) = self.global_to_shard(node)?;
        self.shards.get(shard_idx)
    }

    /// Converts a global NodeId to (shard_index, local_offset).
    #[inline]
    pub fn global_to_shard(&self, node: NodeId) -> Option<(usize, usize)> {
        let id = node.as_u64() as usize;
        for (idx, shard) in self.shards.iter().enumerate() {
            if shard.contains(id) {
                return Some((idx, id - shard.node_range().start));
            }
        }
        None
    }

    /// Converts (shard_index, local_offset) to a global NodeId.
    #[inline]
    pub fn shard_to_global(&self, shard_idx: usize, offset: usize) -> Option<NodeId> {
        self.shards.get(shard_idx).map(|shard| {
            let global = shard.node_range().start + offset;
            NodeId::new(global as u64)
        })
    }

    /// Returns the out-degree of a node (number of outgoing edges).
    pub fn out_degree(&self, node: NodeId) -> usize {
        let base_degree = self
            .shard_for(node)
            .map(|shard| {
                let offset = node.as_u64() as usize - shard.node_range().start;
                shard.out_degree(offset)
            })
            .unwrap_or(0);

        // Account for delta layer
        let delta_insertions = self.delta.insertion_count_for(node);
        let delta_deletions = self.delta.deletion_count_for(node);

        base_degree + delta_insertions - delta_deletions.min(base_degree)
    }

    /// Returns an iterator over the neighbors of a node.
    pub fn neighbors(&self, node: NodeId) -> NeighborIter<'_> {
        NeighborIter {
            graph: self,
            node,
            base_iter: self.base_neighbors(node),
            delta_iter: self.delta.neighbors(node),
        }
    }

    /// Returns an iterator over base layer neighbors only.
    fn base_neighbors(&self, node: NodeId) -> BaseNeighborIter<'_> {
        if let Some(shard) = self.shard_for(node) {
            let offset = node.as_u64() as usize - shard.node_range().start;
            BaseNeighborIter::new(shard, offset)
        } else {
            BaseNeighborIter::empty()
        }
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
            Some(NodeId::new(neighbor as u64))
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
}

impl Iterator for NeighborIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        // First, yield base layer neighbors (skip tombstoned)
        for neighbor in self.base_iter.by_ref() {
            if !self.graph.delta.is_deleted(self.node, neighbor) {
                return Some(neighbor);
            }
        }

        // Then yield delta insertions
        self.delta_iter.next()
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
        let neighbors: Vec<_> = graph.neighbors(NodeId::new(0)).collect();
        assert_eq!(neighbors.len(), 3);
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
}
