//! `FusionGraph` `DataFusion` Integration
//!
//! This crate provides `DataFusion` integration for `FusionGraph`:
//! - `GraphTableProvider` implementation of `TableProvider`
//! - `CSRBuilderExec` physical operator
//! - `GraphTraversalExec` physical operator
//! - Optimizer rules for graph query patterns

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod error;
pub mod exec;
pub mod provider;

pub use error::DataFusionError;
pub use exec::{CSRBuilderExec, GraphTraversalExec};
pub use provider::GraphTableProvider;
