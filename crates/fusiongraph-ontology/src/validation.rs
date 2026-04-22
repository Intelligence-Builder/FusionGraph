//! Ontology validation.

use std::collections::HashSet;

use crate::error::OntologyError;
use crate::schema::Ontology;

/// Structured validation error details for lossless error conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationErrorKind {
    /// Edge references an undefined node label.
    DanglingEdge {
        /// Label of the edge definition with the dangling reference.
        edge: String,
        /// Missing node label referenced by the edge.
        node: String,
    },
    /// Duplicate node or edge label.
    DuplicateLabel {
        /// Duplicate label kind (`node` or `edge`).
        kind: &'static str,
        /// The duplicated label value.
        label: String,
    },
    /// Computed property does not target exactly one graph object.
    InvalidComputedPropertyTarget {
        /// Computed property name.
        property: String,
        /// Referenced node label, if any.
        node: Option<String>,
        /// Referenced edge label, if any.
        edge: Option<String>,
    },
}

/// Validation error (blocking).
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Error code.
    pub code: &'static str,
    /// Error message.
    pub message: String,
    /// Structured validation context.
    pub kind: ValidationErrorKind,
}

/// Validation warning (non-blocking).
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// Warning code.
    pub code: &'static str,
    /// Warning message.
    pub message: String,
}

/// Result of ontology validation.
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    /// Errors (must be fixed).
    pub errors: Vec<ValidationError>,
    /// Warnings (should be fixed).
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    /// Returns true if validation passed (no errors).
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

impl Ontology {
    /// Validates the ontology structure.
    #[must_use]
    pub fn validate(&self) -> ValidationResult {
        let mut result = ValidationResult::default();

        // Check for duplicate node labels
        let mut node_labels = HashSet::new();
        for node in &self.nodes {
            if !node_labels.insert(&node.label) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E004",
                    message: format!("Duplicate node label '{}'", node.label),
                    kind: ValidationErrorKind::DuplicateLabel {
                        kind: "node",
                        label: node.label.clone(),
                    },
                });
            }
        }

        // Check for duplicate edge labels
        let mut edge_labels = HashSet::new();
        for edge in &self.edges {
            if !edge_labels.insert(&edge.label) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E004",
                    message: format!("Duplicate edge label '{}'", edge.label),
                    kind: ValidationErrorKind::DuplicateLabel {
                        kind: "edge",
                        label: edge.label.clone(),
                    },
                });
            }
        }

        // Check for dangling edge references
        for edge in &self.edges {
            if !node_labels.contains(&edge.from_node) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E003",
                    message: format!(
                        "Edge '{}' references undefined node '{}'",
                        edge.label, edge.from_node
                    ),
                    kind: ValidationErrorKind::DanglingEdge {
                        edge: edge.label.clone(),
                        node: edge.from_node.clone(),
                    },
                });
            }
            if !node_labels.contains(&edge.to_node) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E003",
                    message: format!(
                        "Edge '{}' references undefined node '{}'",
                        edge.label, edge.to_node
                    ),
                    kind: ValidationErrorKind::DanglingEdge {
                        edge: edge.label.clone(),
                        node: edge.to_node.clone(),
                    },
                });
            }
        }

        // Check computed properties target exactly one graph object
        for property in &self.properties {
            if property.node.is_some() == property.edge.is_some() {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E007",
                    message: format!(
                        "Computed property '{}' must target exactly one of node or edge",
                        property.name
                    ),
                    kind: ValidationErrorKind::InvalidComputedPropertyTarget {
                        property: property.name.clone(),
                        node: property.node.clone(),
                        edge: property.edge.clone(),
                    },
                });
            }
        }

        // Check for missing partition hints on temporal edges
        for edge in &self.edges {
            if edge.valid_from_column.is_some() && edge.partition_column.is_none() {
                result.warnings.push(ValidationWarning {
                    code: "FG-ONT-W002",
                    message: format!(
                        "Temporal edge '{}' has no partition hint; full scans may occur",
                        edge.label
                    ),
                });
            }
        }

        result
    }

    /// Validates and returns an error if invalid.
    ///
    /// # Errors
    ///
    /// Returns the first blocking ontology validation error with its structured
    /// context preserved.
    pub fn validate_or_error(&self) -> Result<(), OntologyError> {
        let result = self.validate();
        let Some(err) = result.errors.first() else {
            return Ok(());
        };

        Err(match &err.kind {
            ValidationErrorKind::DanglingEdge { edge, node } => OntologyError::DanglingEdge {
                edge: edge.clone(),
                node: node.clone(),
            },
            ValidationErrorKind::DuplicateLabel { kind, label } => OntologyError::DuplicateLabel {
                kind: (*kind).to_string(),
                label: label.clone(),
            },
            ValidationErrorKind::InvalidComputedPropertyTarget {
                property,
                node,
                edge,
            } => OntologyError::InvalidComputedPropertyTarget {
                property: property.clone(),
                node: node.clone(),
                edge: edge.clone(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{parser::parse_toml, OntologyError};

    const VALID_ONTOLOGY: &str = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[nodes]]
label = "Group"
source = "groups"
id_column = "id"

[[edges]]
label = "BELONGS_TO"
source = "memberships"
from_node = "User"
from_column = "user_id"
to_node = "Group"
to_column = "group_id"
"#;

    #[test]
    fn valid_ontology_passes() {
        let ontology = parse_toml(VALID_ONTOLOGY).unwrap();
        let result = ontology.validate();
        assert!(result.is_valid());
    }

    #[test]
    fn duplicate_node_label_fails() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[nodes]]
label = "User"
source = "users2"
id_column = "id"
"#;
        let ontology = parse_toml(toml).unwrap();
        let result = ontology.validate();
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.code == "FG-ONT-E004"));
    }

    #[test]
    fn dangling_edge_fails() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[edges]]
label = "BELONGS_TO"
source = "memberships"
from_node = "User"
from_column = "user_id"
to_node = "NonExistent"
to_column = "group_id"
"#;
        let ontology = parse_toml(toml).unwrap();
        let result = ontology.validate();
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.code == "FG-ONT-E003"));
    }

    #[test]
    fn temporal_without_partition_warns() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[nodes]]
label = "Resource"
source = "resources"
id_column = "id"

[[edges]]
label = "ACCESSED"
source = "access_logs"
from_node = "User"
from_column = "user_id"
to_node = "Resource"
to_column = "resource_id"
valid_from_column = "event_time"
"#;
        let ontology = parse_toml(toml).unwrap();
        let result = ontology.validate();
        assert!(result.is_valid()); // Still valid, just warns
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "FG-ONT-W002"));
    }

    #[test]
    fn validate_or_error_preserves_dangling_edge_context() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[edges]]
label = "BELONGS_TO"
source = "memberships"
from_node = "User"
from_column = "user_id"
to_node = "Group"
to_column = "group_id"
"#;

        let ontology = parse_toml(toml).unwrap();
        let err = ontology.validate_or_error().unwrap_err();

        assert!(matches!(
            err,
            OntologyError::DanglingEdge { edge, node }
                if edge == "BELONGS_TO" && node == "Group"
        ));
    }

    #[test]
    fn validate_or_error_preserves_duplicate_label_context() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[nodes]]
label = "User"
source = "users_archive"
id_column = "id"
"#;

        let ontology = parse_toml(toml).unwrap();
        let err = ontology.validate_or_error().unwrap_err();

        assert!(matches!(
            err,
            OntologyError::DuplicateLabel { kind, label }
                if kind == "node" && label == "User"
        ));
    }

    #[test]
    fn computed_property_with_both_targets_fails() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[edges]]
label = "BELONGS_TO"
source = "memberships"
from_node = "User"
from_column = "user_id"
to_node = "User"
to_column = "manager_id"

[[properties]]
name = "weight"
node = "User"
edge = "BELONGS_TO"
expression = "1"
"#;

        let ontology = parse_toml(toml).unwrap();
        let err = ontology.validate_or_error().unwrap_err();

        assert!(matches!(
            err,
            OntologyError::InvalidComputedPropertyTarget { property, node, edge }
                if property == "weight"
                    && node.as_deref() == Some("User")
                    && edge.as_deref() == Some("BELONGS_TO")
        ));
    }

    #[test]
    fn computed_property_without_target_fails() {
        let toml = r#"
[[nodes]]
label = "User"
source = "users"
id_column = "id"

[[properties]]
name = "risk_score"
expression = "42"
"#;

        let ontology = parse_toml(toml).unwrap();
        let result = ontology.validate();

        assert!(result.errors.iter().any(|err| err.code == "FG-ONT-E007"));
    }
}
