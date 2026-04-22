//! CSR Shard - A single partition of the graph topology.

use std::ops::Range;
use std::sync::Arc;

/// A single shard of the CSR graph.
///
/// Each shard covers a contiguous range of node IDs and stores their
/// outgoing edges in CSR format.
#[derive(Debug)]
pub struct CsrShard {
    /// Shard identifier.
    id: u32,
    /// Range of global node IDs covered by this shard.
    node_range: Range<usize>,
    /// CSR row pointers (offsets into col_indices for each node).
    /// Length: node_count + 1
    row_ptrs: Arc<[u32]>,
    /// CSR column indices (target node IDs).
    col_indices: Arc<[u32]>,
    /// Optional edge weights.
    weights: Option<Arc<[f32]>>,
}

impl CsrShard {
    /// Creates a new shard from CSR arrays.
    pub fn new(
        id: u32,
        node_range: Range<usize>,
        row_ptrs: Arc<[u32]>,
        col_indices: Arc<[u32]>,
        weights: Option<Arc<[f32]>>,
    ) -> Self {
        debug_assert_eq!(row_ptrs.len(), node_range.len() + 1);
        if let Some(ref w) = weights {
            debug_assert_eq!(w.len(), col_indices.len());
        }

        Self {
            id,
            node_range,
            row_ptrs,
            col_indices,
            weights,
        }
    }

    /// Returns the shard ID.
    #[inline]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Returns the range of global node IDs in this shard.
    #[inline]
    pub fn node_range(&self) -> &Range<usize> {
        &self.node_range
    }

    /// Returns the number of nodes in this shard.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.node_range.len()
    }

    /// Returns the number of edges in this shard.
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.col_indices.len()
    }

    /// Returns true if this shard contains the given global node ID.
    #[inline]
    pub fn contains(&self, global_node_id: usize) -> bool {
        self.node_range.contains(&global_node_id)
    }

    /// Returns the out-degree of a node (by local offset).
    #[inline]
    pub fn out_degree(&self, local_offset: usize) -> usize {
        if local_offset >= self.node_count() {
            return 0;
        }
        let start = self.row_ptrs[local_offset] as usize;
        let end = self.row_ptrs[local_offset + 1] as usize;
        end - start
    }

    /// Returns the range of indices in col_indices for a node's neighbors.
    #[inline]
    pub fn neighbor_range(&self, local_offset: usize) -> (usize, usize) {
        if local_offset >= self.node_count() {
            return (0, 0);
        }
        let start = self.row_ptrs[local_offset] as usize;
        let end = self.row_ptrs[local_offset + 1] as usize;
        (start, end)
    }

    /// Returns the column index (target node ID) at the given position.
    #[inline]
    pub fn col_index(&self, idx: usize) -> Option<u32> {
        self.col_indices.get(idx).copied()
    }

    /// Returns the edge weight at the given position.
    #[inline]
    pub fn weight(&self, idx: usize) -> Option<f32> {
        self.weights.as_ref().and_then(|w| w.get(idx).copied())
    }

    /// Returns the memory usage of this shard in bytes.
    pub fn memory_usage(&self) -> usize {
        let row_ptrs_size = self.row_ptrs.len() * std::mem::size_of::<u32>();
        let col_indices_size = self.col_indices.len() * std::mem::size_of::<u32>();
        let weights_size = self
            .weights
            .as_ref()
            .map(|w| w.len() * std::mem::size_of::<f32>())
            .unwrap_or(0);

        row_ptrs_size + col_indices_size + weights_size + std::mem::size_of::<Self>()
    }

    /// Computes a checksum for corruption detection.
    pub fn checksum(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        self.id.hash(&mut hasher);
        self.node_range.start.hash(&mut hasher);
        self.node_range.end.hash(&mut hasher);

        for &ptr in self.row_ptrs.iter() {
            ptr.hash(&mut hasher);
        }
        for &idx in self.col_indices.iter() {
            idx.hash(&mut hasher);
        }

        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_shard() -> CsrShard {
        // Graph: 0 -> [1, 2], 1 -> [2], 2 -> []
        // row_ptrs: [0, 2, 3, 3]
        // col_indices: [1, 2, 2]
        CsrShard::new(
            0,
            0..3,
            Arc::from([0u32, 2, 3, 3]),
            Arc::from([1u32, 2, 2]),
            None,
        )
    }

    #[test]
    fn shard_basics() {
        let shard = make_test_shard();
        assert_eq!(shard.id(), 0);
        assert_eq!(shard.node_count(), 3);
        assert_eq!(shard.edge_count(), 3);
    }

    #[test]
    fn shard_contains() {
        let shard = make_test_shard();
        assert!(shard.contains(0));
        assert!(shard.contains(2));
        assert!(!shard.contains(3));
    }

    #[test]
    fn shard_out_degree() {
        let shard = make_test_shard();
        assert_eq!(shard.out_degree(0), 2);
        assert_eq!(shard.out_degree(1), 1);
        assert_eq!(shard.out_degree(2), 0);
    }

    #[test]
    fn shard_neighbor_range() {
        let shard = make_test_shard();
        assert_eq!(shard.neighbor_range(0), (0, 2));
        assert_eq!(shard.neighbor_range(1), (2, 3));
        assert_eq!(shard.neighbor_range(2), (3, 3));
    }

    #[test]
    fn shard_checksum_consistent() {
        let shard = make_test_shard();
        let c1 = shard.checksum();
        let c2 = shard.checksum();
        assert_eq!(c1, c2);
    }
}
