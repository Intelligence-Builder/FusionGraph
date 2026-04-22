//! Physical execution operators for `FusionGraph`.

mod csr_builder;
mod graph_traversal;

pub use csr_builder::CSRBuilderExec;
pub use graph_traversal::GraphTraversalExec;
