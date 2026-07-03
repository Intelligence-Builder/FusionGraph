//! Node-key dictionary: maps original (string) node identifiers to the
//! dense `u64` IDs the CSR kernel requires, with full reverse mapping.
//!
//! The dense CSR representation cannot address sparse hash spaces (a
//! `u32`/`u64` hash used directly as a node ID would imply a node count of
//! the hash range). Dictionary encoding subsumes the hash-based
//! `IdTransform`s (`HashU64`, `HashU32`, `UuidToU128`): every distinct key
//! gets the next dense ID, deterministically within a build, collision-free,
//! and reversible. Join traversal results back to original keys via the
//! `graph_nodes('name')` table function.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema, SchemaRef};

/// Bidirectional mapping between original node keys and dense `u64` IDs.
///
/// Built during graph projection (insertion order assigns IDs 0..N), then
/// frozen behind an `Arc` in the
/// [`GraphCatalog`](crate::GraphCatalog).
#[derive(Debug, Default)]
pub struct NodeDictionary {
    to_id: HashMap<String, u64>,
    keys: Vec<String>,
}

impl NodeDictionary {
    /// Creates an empty dictionary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the dense ID for `key`, assigning the next ID if unseen.
    pub fn get_or_insert(&mut self, key: &str) -> u64 {
        if let Some(&id) = self.to_id.get(key) {
            return id;
        }
        let id = self.keys.len() as u64;
        self.to_id.insert(key.to_string(), id);
        self.keys.push(key.to_string());
        id
    }

    /// Returns the dense ID for `key`, if present.
    #[must_use]
    pub fn id_of(&self, key: &str) -> Option<u64> {
        self.to_id.get(key).copied()
    }

    /// Returns the original key for a dense ID, if present.
    #[must_use]
    pub fn key_of(&self, id: u64) -> Option<&str> {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.keys.get(i))
            .map(String::as_str)
    }

    /// Number of distinct keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns true if no keys have been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The schema served by `graph_nodes('name')`:
    /// `node_id UInt64, node_key Utf8`.
    #[must_use]
    pub fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("node_id", DataType::UInt64, false),
            Field::new("node_key", DataType::Utf8, false),
        ]))
    }

    /// Materializes the mapping as a record batch (for the `graph_nodes`
    /// table function).
    ///
    /// # Errors
    ///
    /// Returns an error if batch construction fails (schema mismatch is
    /// impossible by construction; arrow allocation failures surface here).
    pub fn to_batch(&self) -> Result<RecordBatch, arrow_schema::ArrowError> {
        let ids: UInt64Array = (0..self.keys.len() as u64).collect::<Vec<_>>().into();
        let keys: StringArray = self
            .keys
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .into();
        RecordBatch::try_new(Self::schema(), vec![Arc::new(ids), Arc::new(keys)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_deterministic_and_reversible() {
        let mut dict = NodeDictionary::new();
        assert_eq!(dict.get_or_insert("alice"), 0);
        assert_eq!(dict.get_or_insert("bob"), 1);
        assert_eq!(dict.get_or_insert("alice"), 0, "idempotent");

        assert_eq!(dict.id_of("bob"), Some(1));
        assert_eq!(dict.id_of("carol"), None);
        assert_eq!(dict.key_of(0), Some("alice"));
        assert_eq!(dict.key_of(9), None);
        assert_eq!(dict.len(), 2);
    }

    #[test]
    fn batch_roundtrip() {
        let mut dict = NodeDictionary::new();
        dict.get_or_insert("a");
        dict.get_or_insert("b");
        let batch = dict.to_batch().unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.schema(), NodeDictionary::schema());
    }
}
