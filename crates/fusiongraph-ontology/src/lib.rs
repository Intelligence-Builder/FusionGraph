//! `FusionGraph` Ontology - Schema definitions for graph projection.
//!
//! This crate provides parsing and validation of ontology schemas that
//! define how relational tables map to graph nodes and edges.

#![warn(missing_docs)]
#![warn(clippy::all)]

mod error;
mod parser;
mod schema;
mod validation;

pub use error::{OntologyError, Result};
pub use schema::{
    ComputedProperty, EdgeDefinition, EdgeDirection, IdColumn, IdTransform, IdType, NodeDefinition,
    Ontology, OntologySettings,
};
pub use validation::{ValidationError, ValidationErrorKind, ValidationWarning};

/// Re-export for convenience.
pub use parser::parse_toml;
