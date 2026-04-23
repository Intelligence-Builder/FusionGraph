//! Lock-free delta layer for real-time graph updates.
//!
//! The delta layer stores insertions and deletions (tombstones) that
//! haven't yet been compacted into the base CSR layer.

use dashmap::{DashMap, DashSet};

use crate::types::{EdgeData, NodeId};

/// Lock-free delta layer for real-time edge updates.
#[derive(Debug)]
pub struct DeltaLayer {
    /// New edges not yet compacted into base layer.
    insertions: DashMap<(NodeId, NodeId), EdgeData>,
    /// Tombstones for deleted edges.
    deletions: DashSet<(NodeId, NodeId)>,
}

impl DeltaLayer {
    /// Creates a new empty delta layer.
    pub fn new() -> Self {
        Self {
            insertions: DashMap::new(),
            deletions: DashSet::new(),
        }
    }

    /// Inserts an edge into the delta layer.
    pub fn insert(&self, from: NodeId, to: NodeId, data: EdgeData) {
        // Remove any existing tombstone
        self.deletions.remove(&(from, to));
        // Insert the edge
        self.insertions.insert((from, to), data);
    }

    /// Deletes an edge (creates a tombstone).
    pub fn delete(&self, from: NodeId, to: NodeId) {
        // Remove from insertions if present
        self.insertions.remove(&(from, to));
        // Add tombstone
        self.deletions.insert((from, to));
    }

    /// Returns true if the edge has a tombstone (is deleted).
    #[inline]
    pub fn is_deleted(&self, from: NodeId, to: NodeId) -> bool {
        self.deletions.contains(&(from, to))
    }

    /// Returns true if the edge exists in the insertion set.
    #[inline]
    pub fn has_insertion(&self, from: NodeId, to: NodeId) -> bool {
        self.insertions.contains_key(&(from, to))
    }

    /// Returns the number of pending insertions for a specific source node.
    pub fn insertion_count_for(&self, from: NodeId) -> usize {
        self.insertions
            .iter()
            .filter(|entry| entry.key().0 == from)
            .count()
    }

    /// Returns the number of pending deletions for a specific source node.
    pub fn deletion_count_for(&self, from: NodeId) -> usize {
        self.deletions
            .iter()
            .filter(|entry| entry.0 == from)
            .count()
    }

    /// Returns neighbors added via delta insertions.
    pub fn neighbors(&self, from: NodeId) -> std::vec::IntoIter<NodeId> {
        let neighbors: Vec<_> = self
            .insertions
            .iter()
            .filter_map(|entry| {
                if entry.key().0 == from {
                    Some(entry.key().1)
                } else {
                    None
                }
            })
            .collect();
        neighbors.into_iter()
    }

    /// Returns the total number of entries (insertions + deletions).
    pub fn len(&self) -> usize {
        self.insertions.len() + self.deletions.len()
    }

    /// Returns true if the delta layer is empty.
    pub fn is_empty(&self) -> bool {
        self.insertions.is_empty() && self.deletions.is_empty()
    }

    /// Returns the number of pending insertions.
    pub fn insertion_count(&self) -> usize {
        self.insertions.len()
    }

    /// Returns the number of pending deletions.
    pub fn deletion_count(&self) -> usize {
        self.deletions.len()
    }

    /// Clears all entries from the delta layer.
    pub fn clear(&self) {
        self.insertions.clear();
        self.deletions.clear();
    }

    /// Returns approximate memory usage in bytes.
    pub fn memory_usage(&self) -> usize {
        let insertion_size = self.insertions.len()
            * (std::mem::size_of::<(NodeId, NodeId)>() + std::mem::size_of::<EdgeData>());
        let deletion_size = self.deletions.len() * std::mem::size_of::<(NodeId, NodeId)>();
        insertion_size + deletion_size + std::mem::size_of::<Self>()
    }

    /// Drains all insertions for compaction.
    pub fn drain_insertions(&self) -> Vec<((NodeId, NodeId), EdgeData)> {
        let entries: Vec<_> = self
            .insertions
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        for ((from, to), _) in &entries {
            self.insertions.remove(&(*from, *to));
        }

        entries
    }

    /// Drains all deletions for compaction.
    pub fn drain_deletions(&self) -> Vec<(NodeId, NodeId)> {
        let entries: Vec<_> = self.deletions.iter().map(|entry| *entry).collect();

        for (from, to) in &entries {
            self.deletions.remove(&(*from, *to));
        }

        entries
    }
}

impl Default for DeltaLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query() {
        let delta = DeltaLayer::new();
        delta.insert(NodeId(0), NodeId(1), EdgeData::default());

        assert!(delta.has_insertion(NodeId(0), NodeId(1)));
        assert!(!delta.has_insertion(NodeId(1), NodeId(0)));
        assert_eq!(delta.insertion_count(), 1);
    }

    #[test]
    fn delete_creates_tombstone() {
        let delta = DeltaLayer::new();
        delta.delete(NodeId(0), NodeId(1));

        assert!(delta.is_deleted(NodeId(0), NodeId(1)));
        assert!(!delta.is_deleted(NodeId(1), NodeId(0)));
        assert_eq!(delta.deletion_count(), 1);
    }

    #[test]
    fn insert_removes_tombstone() {
        let delta = DeltaLayer::new();
        delta.delete(NodeId(0), NodeId(1));
        assert!(delta.is_deleted(NodeId(0), NodeId(1)));

        delta.insert(NodeId(0), NodeId(1), EdgeData::default());
        assert!(!delta.is_deleted(NodeId(0), NodeId(1)));
        assert!(delta.has_insertion(NodeId(0), NodeId(1)));
    }

    #[test]
    fn delete_removes_insertion() {
        let delta = DeltaLayer::new();
        delta.insert(NodeId(0), NodeId(1), EdgeData::default());
        assert!(delta.has_insertion(NodeId(0), NodeId(1)));

        delta.delete(NodeId(0), NodeId(1));
        assert!(!delta.has_insertion(NodeId(0), NodeId(1)));
        assert!(delta.is_deleted(NodeId(0), NodeId(1)));
    }

    #[test]
    fn neighbors_iteration() {
        let delta = DeltaLayer::new();
        delta.insert(NodeId(0), NodeId(1), EdgeData::default());
        delta.insert(NodeId(0), NodeId(2), EdgeData::default());
        delta.insert(NodeId(1), NodeId(2), EdgeData::default());

        let neighbors: Vec<_> = delta.neighbors(NodeId(0)).collect();
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors.contains(&NodeId(1)));
        assert!(neighbors.contains(&NodeId(2)));
    }

    #[test]
    fn concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let delta = Arc::new(DeltaLayer::new());
        let mut handles = vec![];

        for t in 0u64..4 {
            let delta = Arc::clone(&delta);
            handles.push(thread::spawn(move || {
                for i in 0u64..1000 {
                    let from = NodeId(t * 1000 + i);
                    let to = NodeId(i);
                    delta.insert(from, to, EdgeData::default());
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(delta.insertion_count(), 4000);
    }
}
