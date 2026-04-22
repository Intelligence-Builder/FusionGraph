//! Ontology validation.

use std::collections::HashSet;

use crate::error::OntologyError;
use crate::schema::Ontology;

/// Validation error (blocking).
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Error code.
    pub code: &'static str,
    /// Error message.
    pub message: String,
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
    pub fn validate(&self) -> ValidationResult {
        let mut result = ValidationResult::default();

        // Check for duplicate node labels
        let mut node_labels = HashSet::new();
        for node in &self.nodes {
            if !node_labels.insert(&node.label) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E004",
                    message: format!("Duplicate node label '{}'", node.label),
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
                });
            }
            if !node_labels.contains(&edge.to_node) {
                result.errors.push(ValidationError {
                    code: "FG-ONT-E003",
                    message: format!(
                        "Edge '{}' references undefined node '{}'",
                        edge.label, edge.to_node
                    ),
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
    pub fn validate_or_error(&self) -> Result<(), OntologyError> {
        let result = self.validate();
        if let Some(err) = result.errors.first() {
            // Convert first error to OntologyError
            if err.code == "FG-ONT-E003" {
                // Parse edge and node from message
                return Err(OntologyError::DanglingEdge {
                    edge: "unknown".to_string(),
                    node: "unknown".to_string(),
                });
            }
            if err.code == "FG-ONT-E004" {
                return Err(OntologyError::DuplicateLabel {
                    kind: "node".to_string(),
                    label: "unknown".to_string(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::parse_toml;

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
}
