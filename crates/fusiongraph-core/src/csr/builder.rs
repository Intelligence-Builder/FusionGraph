//! CSR Builder - Constructs CSR graphs from edge streams.

use std::sync::Arc;

use crate::delta::DeltaLayer;
use crate::error::Result;

use super::{CsrGraph, CsrShard, DEFAULT_SHARD_SIZE};

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
    pub fn with_weighted_edges(
        mut self,
        edges: impl IntoIterator<Item = (u64, u64, f32)>,
    ) -> Self {
        self.config.include_weights = true;
        for (src, dst, weight) in edges {
            self.edges.push((src, dst));
            self.weights.push(weight);
        }
        self
    }

    /// Builds the CSR graph.
    pub fn build(mut self) -> Result<CsrGraph> {
        if self.edges.is_empty() {
            return Ok(CsrGraph::empty());
        }

        // Sort edges by source node for CSR construction
        if self.config.include_weights {
            let mut edge_weights: Vec<_> =
                self.edges.iter().copied().zip(self.weights.iter().copied()).collect();
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
        let node_count = (max_node + 1) as usize;

        // Build CSR arrays
        let (row_ptrs, col_indices, weights) = self.build_csr_arrays(node_count);

        // For now, create a single shard (multi-shard logic to be added in #6)
        let shard = CsrShard::new(
            0,
            0..node_count,
            Arc::from(row_ptrs),
            Arc::from(col_indices),
            weights.map(Arc::from),
        );

        Ok(CsrGraph {
            shards: vec![Arc::new(shard)],
            node_count,
            edge_count: self.edges.len(),
            delta: Arc::new(DeltaLayer::new()),
        })
    }

    /// Builds the raw CSR arrays.
    fn build_csr_arrays(&self, node_count: usize) -> (Vec<u32>, Vec<u32>, Option<Vec<f32>>) {
        // Count edges per node
        let mut degrees = vec![0u32; node_count];
        for &(src, _) in &self.edges {
            degrees[src as usize] += 1;
        }

        // Build row pointers (cumulative sum)
        let mut row_ptrs = Vec::with_capacity(node_count + 1);
        row_ptrs.push(0);
        let mut cumsum = 0u32;
        for &degree in &degrees {
            cumsum += degree;
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
            let src_idx = src as usize;
            let pos = current_pos[src_idx] as usize;
            col_indices[pos] = dst as u32;
            if let Some(ref mut w) = weights {
                w[pos] = self.weights[i];
            }
            current_pos[src_idx] += 1;
        }

        (row_ptrs, col_indices, weights)
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
}
