//! CSR Builder - Constructs CSR graphs from edge streams.

use std::sync::Arc;

use crate::delta::DeltaLayer;
use crate::error::{GraphError, Result};
use crate::types::NodeId;

use super::{CsrGraph, CsrShard, DEFAULT_SHARD_SIZE};

type CsrArrays = (Vec<u32>, Vec<u32>, Option<Vec<f32>>);

/// Configuration for CSR building.
#[derive(Debug, Clone)]
pub struct CsrBuildConfig {
    /// Shard size in bytes.
    pub shard_size: usize,
    /// Whether to include edge weights.
    pub include_weights: bool,
}

impl Default for CsrBuildConfig {
    fn default() -> Self {
        Self {
            shard_size: DEFAULT_SHARD_SIZE,
            include_weights: false,
        }
    }
}

/// Builder for constructing CSR graphs.
#[derive(Debug)]
pub struct CsrBuilder {
    config: CsrBuildConfig,
    edges: Vec<(u64, u64)>,
    weights: Vec<f32>,
}

impl CsrBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: CsrBuildConfig::default(),
            edges: Vec::new(),
            weights: Vec::new(),
        }
    }

    /// Sets the shard size.
    pub fn with_shard_size(mut self, size: usize) -> Self {
        self.config.shard_size = size;
        self
    }

    /// Adds edges from an iterator.
    pub fn with_edges(mut self, edges: impl IntoIterator<Item = (u64, u64)>) -> Self {
        self.edges.extend(edges);
        self
    }

    /// Adds weighted edges from an iterator.
    pub fn with_weighted_edges(mut self, edges: impl IntoIterator<Item = (u64, u64, f32)>) -> Self {
        self.config.include_weights = true;
        for (src, dst, weight) in edges {
            self.edges.push((src, dst));
            self.weights.push(weight);
        }
        self
    }

    /// Builds the CSR graph.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::InvalidEdge`] when an edge endpoint exceeds the
    /// supported `u32` node range and [`GraphError::UnsupportedGraphSize`]
    /// when the graph exceeds this implementation's CSR capacity limits.
    pub fn build(mut self) -> Result<CsrGraph> {
        if self.edges.is_empty() {
            return Ok(CsrGraph::empty());
        }

        self.validate_edge_ids()?;

        // Sort edges by source node for CSR construction
        if self.config.include_weights {
            let mut edge_weights: Vec<_> = self
                .edges
                .iter()
                .copied()
                .zip(self.weights.iter().copied())
                .collect();
            edge_weights.sort_by_key(|((src, _), _)| *src);
            self.edges = edge_weights.iter().map(|((s, d), _)| (*s, *d)).collect();
            self.weights = edge_weights.iter().map(|(_, w)| *w).collect();
        } else {
            self.edges.sort_by_key(|(src, _)| *src);
        }

        // Find max node ID to determine node count
        let max_node = self
            .edges
            .iter()
            .flat_map(|(src, dst)| [*src, *dst])
            .max()
            .unwrap_or(0);
        let node_count =
            usize::try_from(max_node + 1).map_err(|_| GraphError::UnsupportedGraphSize {
                reason: format!(
                    "node count {} exceeds platform addressable range",
                    max_node + 1
                ),
            })?;

        if self.edges.len() > u32::MAX as usize {
            return Err(GraphError::UnsupportedGraphSize {
                reason: format!(
                    "edge count {} exceeds CSR row pointer capacity",
                    self.edges.len()
                ),
            });
        }

        // Build CSR arrays
        let (row_ptrs, col_indices, weights) = self.build_csr_arrays(node_count)?;

        // Partition into shards based on configured shard_size
        let shards =
            self.partition_into_shards(node_count, &row_ptrs, &col_indices, weights.as_deref())?;

        Ok(CsrGraph {
            shards,
            node_count,
            edge_count: self.edges.len(),
            delta: Arc::new(DeltaLayer::new()),
        })
    }

    /// Builds the raw CSR arrays.
    fn build_csr_arrays(&self, node_count: usize) -> Result<CsrArrays> {
        // Count edges per node
        let mut degrees = vec![0u32; node_count];
        for &(src, _) in &self.edges {
            let src_idx = Self::edge_src_index(src, node_count)?;
            let degree = degrees
                .get_mut(src_idx)
                .ok_or_else(|| GraphError::InvalidEdge {
                    from: NodeId::new(src),
                    to: NodeId::new(src),
                })?;
            *degree = degree
                .checked_add(1)
                .ok_or_else(|| GraphError::UnsupportedGraphSize {
                    reason: format!("node {src} exceeds per-node CSR degree capacity"),
                })?;
        }

        // Build row pointers (cumulative sum)
        let mut row_ptrs = Vec::with_capacity(node_count + 1);
        row_ptrs.push(0);
        let mut cumsum = 0u32;
        for &degree in &degrees {
            cumsum =
                cumsum
                    .checked_add(degree)
                    .ok_or_else(|| GraphError::UnsupportedGraphSize {
                        reason: "total edge count exceeds CSR row pointer capacity".to_string(),
                    })?;
            row_ptrs.push(cumsum);
        }

        // Build column indices and weights
        let mut col_indices = vec![0u32; self.edges.len()];
        let mut weights = if self.config.include_weights {
            Some(vec![0.0f32; self.edges.len()])
        } else {
            None
        };

        // Track current position for each node
        let mut current_pos = row_ptrs[..node_count].to_vec();

        for (i, &(src, dst)) in self.edges.iter().enumerate() {
            let src_idx = Self::edge_src_index(src, node_count)?;
            let pos = current_pos[src_idx] as usize;
            col_indices[pos] = u32::try_from(dst).map_err(|_| GraphError::InvalidEdge {
                from: NodeId::new(src),
                to: NodeId::new(dst),
            })?;
            if let Some(ref mut w) = weights {
                w[pos] = self.weights[i];
            }
            current_pos[src_idx] = current_pos[src_idx].checked_add(1).ok_or_else(|| {
                GraphError::UnsupportedGraphSize {
                    reason: format!("node {src} exceeds per-node CSR degree capacity"),
                }
            })?;
        }

        Ok((row_ptrs, col_indices, weights))
    }

    /// Partitions the CSR arrays into shards based on the configured shard size.
    ///
    /// Each shard targets approximately `shard_size` bytes of memory, containing
    /// a contiguous range of node IDs with their associated row pointers,
    /// column indices, and optional weights.
    fn partition_into_shards(
        &self,
        node_count: usize,
        row_ptrs: &[u32],
        col_indices: &[u32],
        weights: Option<&[f32]>,
    ) -> Result<Vec<Arc<CsrShard>>> {
        if node_count == 0 {
            return Ok(Vec::new());
        }

        // Calculate memory per edge: 4 bytes col_idx + optional 4 bytes weight
        let bytes_per_edge = if weights.is_some() { 8usize } else { 4usize };
        let row_ptr_size = std::mem::size_of::<u32>();

        // Estimate nodes per shard based on average degree
        let total_edge_bytes = col_indices.len().saturating_mul(bytes_per_edge);
        let total_row_bytes = (node_count + 1).saturating_mul(row_ptr_size);
        let total_bytes = total_row_bytes.saturating_add(total_edge_bytes);
        let avg_bytes_per_node = total_bytes.div_ceil(node_count).max(1);

        // Target nodes per shard (at least 1 node per shard).
        let target_nodes_per_shard = self.config.shard_size.div_ceil(avg_bytes_per_node).max(1);

        let mut shards = Vec::new();
        let mut start_node = 0usize;
        let mut shard_id = 0u32;

        while start_node < node_count {
            // Determine end node for this shard
            let mut end_node = (start_node + target_nodes_per_shard).min(node_count);

            // Adjust based on actual memory usage to stay close to shard_size
            let mut shard_bytes =
                Self::calculate_shard_memory(start_node, end_node, row_ptrs, bytes_per_edge);

            // If we're over the shard size and have more than 1 node, shrink
            while shard_bytes > self.config.shard_size && end_node > start_node + 1 {
                end_node -= 1;
                shard_bytes =
                    Self::calculate_shard_memory(start_node, end_node, row_ptrs, bytes_per_edge);
            }

            // Extract shard data
            let shard_node_count = end_node - start_node;

            // Build shard row pointers (relative to shard start)
            let shard_edge_start = usize::try_from(row_ptrs[start_node]).map_err(|_| {
                GraphError::UnsupportedGraphSize {
                    reason: "row pointer does not fit in usize on this target".to_string(),
                }
            })?;
            let shard_edge_end = usize::try_from(row_ptrs[end_node]).map_err(|_| {
                GraphError::UnsupportedGraphSize {
                    reason: "row pointer does not fit in usize on this target".to_string(),
                }
            })?;

            let mut shard_row_ptrs = Vec::with_capacity(shard_node_count + 1);
            for i in start_node..=end_node {
                let relative_ptr = row_ptrs[i] - row_ptrs[start_node];
                shard_row_ptrs.push(relative_ptr);
            }

            // Extract column indices for this shard
            let shard_col_indices: Vec<u32> =
                col_indices[shard_edge_start..shard_edge_end].to_vec();

            // Extract weights if present
            let shard_weights = weights.map(|w| w[shard_edge_start..shard_edge_end].to_vec());

            let shard = CsrShard::new(
                shard_id,
                start_node..end_node,
                Arc::from(shard_row_ptrs),
                Arc::from(shard_col_indices),
                shard_weights.map(Arc::from),
            );

            shards.push(Arc::new(shard));

            start_node = end_node;
            shard_id = shard_id
                .checked_add(1)
                .ok_or_else(|| GraphError::UnsupportedGraphSize {
                    reason: "shard count exceeds u32::MAX".to_string(),
                })?;
        }

        Ok(shards)
    }

    /// Calculates the memory usage for a potential shard.
    fn calculate_shard_memory(
        start_node: usize,
        end_node: usize,
        row_ptrs: &[u32],
        bytes_per_edge: usize,
    ) -> usize {
        let node_count = end_node - start_node;
        let edge_start = usize::try_from(row_ptrs[start_node]).expect("u32 row pointer fits usize");
        let edge_end = usize::try_from(row_ptrs[end_node]).expect("u32 row pointer fits usize");
        let edge_count = edge_end - edge_start;

        // row_ptrs: (node_count + 1) * 4 bytes
        // col_indices + optional weights: edge_count * bytes_per_edge
        (node_count + 1) * std::mem::size_of::<u32>() + edge_count * bytes_per_edge
    }

    fn validate_edge_ids(&self) -> Result<()> {
        for &(src, dst) in &self.edges {
            if src > u64::from(u32::MAX) || dst > u64::from(u32::MAX) {
                return Err(GraphError::InvalidEdge {
                    from: NodeId::new(src),
                    to: NodeId::new(dst),
                });
            }
        }

        Ok(())
    }

    fn edge_src_index(src: u64, node_count: usize) -> Result<usize> {
        let src_idx = usize::try_from(src).map_err(|_| GraphError::InvalidEdge {
            from: NodeId::new(src),
            to: NodeId::new(src),
        })?;

        if src_idx >= node_count {
            return Err(GraphError::InvalidEdge {
                from: NodeId::new(src),
                to: NodeId::new(src),
            });
        }

        Ok(src_idx)
    }
}

impl Default for CsrBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_empty() {
        let graph = CsrBuilder::new().build().unwrap();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn build_simple() {
        let graph = CsrBuilder::new()
            .with_edges([(0, 1), (0, 2), (1, 2)])
            .build()
            .unwrap();

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 3);
    }

    #[test]
    fn build_weighted() {
        let graph = CsrBuilder::new()
            .with_weighted_edges([(0, 1, 1.0), (0, 2, 2.0), (1, 2, 0.5)])
            .build()
            .unwrap();

        assert_eq!(graph.edge_count(), 3);
    }

    #[test]
    fn build_unsorted_edges() {
        // Edges not in order should still work
        let graph = CsrBuilder::new()
            .with_edges([(2, 3), (0, 1), (1, 2)])
            .build()
            .unwrap();

        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.edge_count(), 3);
    }

    #[test]
    fn build_rejects_node_ids_beyond_u32() {
        let err = CsrBuilder::new()
            .with_edges([(0, u64::from(u32::MAX) + 1)])
            .build()
            .unwrap_err();

        assert!(matches!(
            err,
            GraphError::InvalidEdge { from, to }
                if from == NodeId::new(0) && to == NodeId::new(u64::from(u32::MAX) + 1)
        ));
    }

    // =========================================================================
    // Shard Partitioning Tests (Issue #6)
    // =========================================================================

    #[test]
    fn shard_partitioning_creates_multiple_shards_for_small_shard_size() {
        // Create a graph with enough data to require multiple shards
        // Each edge needs ~4 bytes (col_index), plus row_ptrs
        // With shard_size = 32 bytes, we should get multiple shards
        let edges: Vec<(u64, u64)> = (0..20).map(|i| (i, i + 1)).collect();

        let graph = CsrBuilder::new()
            .with_shard_size(32) // Very small shard size to force partitioning
            .with_edges(edges)
            .build()
            .unwrap();

        assert!(
            graph.shard_count() > 1,
            "Expected multiple shards, got {}",
            graph.shard_count()
        );
    }

    #[test]
    fn global_to_shard_roundtrip_single_shard() {
        let graph = CsrBuilder::new()
            .with_edges([(0, 1), (1, 2), (2, 3)])
            .build()
            .unwrap();

        // Test roundtrip for all nodes
        for node_id in 0..4 {
            let node = NodeId::new(node_id);
            if let Some((shard_idx, offset)) = graph.global_to_shard(node) {
                let recovered = graph.shard_to_global(shard_idx, offset);
                assert_eq!(
                    recovered,
                    Some(node),
                    "Roundtrip failed for node {}",
                    node_id
                );
            }
        }
    }

    #[test]
    fn global_to_shard_roundtrip_multiple_shards() {
        // Force multiple shards with small shard size
        let edges: Vec<(u64, u64)> = (0..50).map(|i| (i, i + 1)).collect();

        let graph = CsrBuilder::new()
            .with_shard_size(64) // Small enough to create multiple shards
            .with_edges(edges)
            .build()
            .unwrap();

        // Test roundtrip for all nodes across all shards
        for node_id in 0..51 {
            let node = NodeId::new(node_id);
            let result = graph.global_to_shard(node);
            assert!(
                result.is_some(),
                "global_to_shard failed for node {}",
                node_id
            );

            let (shard_idx, offset) = result.unwrap();
            let recovered = graph.shard_to_global(shard_idx, offset);
            assert_eq!(
                recovered,
                Some(node),
                "Roundtrip failed for node {} (shard={}, offset={})",
                node_id,
                shard_idx,
                offset
            );
        }
    }

    #[test]
    fn cross_shard_neighbor_access() {
        // Create edges that cross shard boundaries
        // Node 0 -> [10, 20, 30, 40] - neighbors likely in different shards
        let mut edges: Vec<(u64, u64)> = vec![(0, 10), (0, 20), (0, 30), (0, 40)];

        // Add filler edges to create more nodes
        for i in 1..50 {
            edges.push((i, i + 1));
        }

        let graph = CsrBuilder::new()
            .with_shard_size(64)
            .with_edges(edges)
            .build()
            .unwrap();

        // Node 0's neighbors should still be accessible
        let neighbors: Vec<_> = graph.neighbors(NodeId::new(0)).collect();

        assert!(
            neighbors.contains(&NodeId::new(10)),
            "Missing cross-shard neighbor 10"
        );
        assert!(
            neighbors.contains(&NodeId::new(20)),
            "Missing cross-shard neighbor 20"
        );
        assert!(
            neighbors.contains(&NodeId::new(30)),
            "Missing cross-shard neighbor 30"
        );
        assert!(
            neighbors.contains(&NodeId::new(40)),
            "Missing cross-shard neighbor 40"
        );
    }

    #[test]
    fn shard_boundary_nodes_handled_correctly() {
        // Create a graph where we can verify boundary behavior
        let edges: Vec<(u64, u64)> = (0..100).map(|i| (i, i + 1)).collect();

        let graph = CsrBuilder::new()
            .with_shard_size(128)
            .with_edges(edges)
            .build()
            .unwrap();

        let shard_count = graph.shard_count();
        assert!(shard_count >= 1);

        // Verify each shard's range is contiguous with the next
        let mut covered_nodes = 0usize;
        for shard_idx in 0..shard_count {
            if let Some(shard) = graph.shards().get(shard_idx) {
                let range = shard.node_range();

                // Range should start where previous ended
                assert_eq!(
                    range.start, covered_nodes,
                    "Gap in shard coverage at shard {}",
                    shard_idx
                );

                covered_nodes = range.end;
            }
        }

        // All nodes should be covered
        assert_eq!(
            covered_nodes,
            graph.node_count(),
            "Not all nodes covered by shards"
        );
    }

    #[test]
    fn shard_partitioning_preserves_edge_count() {
        let edges: Vec<(u64, u64)> = (0..100).flat_map(|i| [(i, i + 1), (i, i + 2)]).collect();

        let graph = CsrBuilder::new()
            .with_shard_size(64)
            .with_edges(edges.clone())
            .build()
            .unwrap();

        // Total edges across all shards should match input
        let total_shard_edges: usize = (0..graph.shard_count())
            .filter_map(|idx| graph.shards().get(idx))
            .map(|s| s.edge_count())
            .sum();

        assert_eq!(
            total_shard_edges,
            edges.len(),
            "Edge count mismatch after sharding"
        );
    }

    #[test]
    fn global_to_shard_returns_none_for_invalid_node() {
        let graph = CsrBuilder::new()
            .with_edges([(0, 1), (1, 2)])
            .build()
            .unwrap();

        // Node 100 doesn't exist
        assert!(graph.global_to_shard(NodeId::new(100)).is_none());
    }

    #[test]
    fn shard_to_global_returns_none_for_invalid_shard() {
        let graph = CsrBuilder::new()
            .with_edges([(0, 1), (1, 2)])
            .build()
            .unwrap();

        // Invalid shard index
        assert!(graph.shard_to_global(999, 0).is_none());
    }

    #[test]
    fn shard_to_global_returns_none_for_invalid_offset() {
        let graph = CsrBuilder::new()
            .with_edges([(0, 1), (1, 2)])
            .build()
            .unwrap();
        let shard = &graph.shards()[0];

        assert!(graph.shard_to_global(0, shard.node_count()).is_none());
    }

    #[test]
    fn weighted_graph_shard_partitioning() {
        // Test that weighted graphs partition correctly
        let edges: Vec<(u64, u64, f32)> = (0_u16..50)
            .map(|i| (u64::from(i), u64::from(i + 1), f32::from(i) * 0.1))
            .collect();

        let graph = CsrBuilder::new()
            .with_shard_size(128)
            .with_weighted_edges(edges)
            .build()
            .unwrap();

        // Verify roundtrip works for weighted graph
        for node_id in 0..51 {
            let node = NodeId::new(node_id);
            if let Some((shard_idx, offset)) = graph.global_to_shard(node) {
                let recovered = graph.shard_to_global(shard_idx, offset);
                assert_eq!(recovered, Some(node));
            }
        }
    }
}
