//! FusionGraph Core - High-performance graph kernel
//!
//! This crate currently provides the core graph data structures and APIs for:
//! - CSR (Compressed Sparse Row) storage with micro-sharding
//! - A delta layer for real-time updates
//! - Breadth-first traversal (BFS)
//!
//! Planned extensions such as additional traversal algorithms (for example, DFS
//! and Dijkstra) and epoch-based memory reclamation are not yet exposed through
//! this crate's public API.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod csr;
pub mod delta;
pub mod error;
pub mod traversal;
pub mod types;

pub use csr::{CsrGraph, CsrShard};
pub use delta::DeltaLayer;
pub use error::GraphError;
pub use traversal::{bfs, TraversalResult};
pub use types::{EdgeId, GraphStatistics, NodeId};
