//! `FusionGraph` Core - High-performance graph kernel
//!
//! This crate provides the core graph data structures and algorithms:
//! - CSR (Compressed Sparse Row) storage with micro-sharding
//! - Lock-free delta layer for real-time updates
//! - Traversal primitives with breadth-first search
//! - Epoch-based memory reclamation

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

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
