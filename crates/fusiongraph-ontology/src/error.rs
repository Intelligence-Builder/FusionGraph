//! Ontology error types.

use thiserror::Error;

/// Errors that can occur during ontology parsing and validation.
#[derive(Error, Debug)]
pub enum OntologyError {
    /// TOML parse error.
    #[error("FG-ONT-E001: Parse error: {message}")]
    ParseError {
        /// Error message.
        message: String,
    },

    /// JSON parse error.
    #[error("FG-ONT-E001: JSON parse error: {message}")]
    JsonError {
        /// Error message.
        message: String,
    },

    /// Missing required field.
    #[error("FG-ONT-E002: Missing required field '{field}' in {context}")]
    MissingField {
        /// Field name.
        field: String,
        /// Context (e.g., "node 'User'").
        context: String,
    },

    /// Dangling edge reference.
    #[error("FG-ONT-E003: Edge '{edge}' references undefined node '{node}'")]
    DanglingEdge {
        /// Edge label.
        edge: String,
        /// Referenced node label.
        node: String,
    },

    /// Duplicate label.
    #[error("FG-ONT-E004: Duplicate {kind} label '{label}'")]
    DuplicateLabel {
        /// "node" or "edge".
        kind: String,
        /// The duplicate label.
        label: String,
    },

    /// Type mismatch.
    #[error(
        "FG-ONT-E005: Cannot apply '{transform}' to column '{column}' of type '{column_type}'"
    )]
    TypeMismatch {
        /// Transform name.
        transform: String,
        /// Column name.
        column: String,
        /// Column type.
        column_type: String,
    },

    /// Computed property target is invalid.
    #[error(
        "FG-ONT-E007: Computed property '{property}' must target exactly one of node or edge (node={node:?}, edge={edge:?})"
    )]
    InvalidComputedPropertyTarget {
        /// Computed property name.
        property: String,
        /// Referenced node label, if any.
        node: Option<String>,
        /// Referenced edge label, if any.
        edge: Option<String>,
    },

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl OntologyError {
    /// Returns the error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ParseError { .. } | Self::JsonError { .. } => "FG-ONT-E001",
            Self::MissingField { .. } => "FG-ONT-E002",
            Self::DanglingEdge { .. } => "FG-ONT-E003",
            Self::DuplicateLabel { .. } => "FG-ONT-E004",
            Self::TypeMismatch { .. } => "FG-ONT-E005",
            Self::InvalidComputedPropertyTarget { .. } => "FG-ONT-E007",
            Self::Io(_) => "FG-ONT-E006",
        }
    }
}

/// Result type alias for ontology operations.
pub type Result<T> = std::result::Result<T, OntologyError>;
