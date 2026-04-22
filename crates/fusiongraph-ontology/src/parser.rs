//! Ontology parser for TOML and JSON formats.

use std::path::Path;

use crate::error::{OntologyError, Result};
use crate::schema::Ontology;

/// Parses an ontology from a TOML string.
pub fn parse_toml(content: &str) -> Result<Ontology> {
    toml::from_str(content).map_err(|e| OntologyError::ParseError {
        message: e.to_string(),
    })
}

/// Parses an ontology from a JSON string.
pub fn parse_json(content: &str) -> Result<Ontology> {
    serde_json::from_str(content).map_err(|e| OntologyError::JsonError {
        message: e.to_string(),
    })
}

/// Parses an ontology from a file (auto-detects format).
pub fn parse_file(path: &Path) -> Result<Ontology> {
    let content = std::fs::read_to_string(path)?;

    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => parse_toml(&content),
        Some("json") => parse_json(&content),
        _ => {
            // Try TOML first, then JSON
            parse_toml(&content).or_else(|_| parse_json(&content))
        }
    }
}

impl Ontology {
    /// Parses from a TOML string.
    pub fn from_toml(content: &str) -> Result<Self> {
        parse_toml(content)
    }

    /// Parses from a JSON string.
    pub fn from_json(content: &str) -> Result<Self> {
        parse_json(content)
    }

    /// Loads from a file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        parse_file(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
[ontology]
name = "test_graph"
version = "1.0"

[settings]
default_node_id_type = "u64"
edge_direction = "directed"

[[nodes]]
label = "User"
source = "iceberg.iam.users"
id_column = "user_id"
properties = ["email", "name"]

[[nodes]]
label = "Group"
source = "iceberg.iam.groups"
id_column = "group_id"

[[edges]]
label = "BELONGS_TO"
source = "iceberg.iam.user_groups"
from_node = "User"
from_column = "user_id"
to_node = "Group"
to_column = "group_id"
"#;

    #[test]
    fn parse_valid_toml() {
        let ontology = parse_toml(EXAMPLE_TOML).unwrap();

        assert_eq!(ontology.name(), "test_graph");
        assert_eq!(ontology.node_labels(), vec!["User", "Group"]);
        assert_eq!(ontology.edge_labels(), vec!["BELONGS_TO"]);
    }

    #[test]
    fn parse_node_properties() {
        let ontology = parse_toml(EXAMPLE_TOML).unwrap();
        let user = ontology.node("User").unwrap();

        assert_eq!(user.properties, vec!["email", "name"]);
    }

    #[test]
    fn parse_edge_references() {
        let ontology = parse_toml(EXAMPLE_TOML).unwrap();
        let edge = ontology.edge("BELONGS_TO").unwrap();

        assert_eq!(edge.from_node, "User");
        assert_eq!(edge.to_node, "Group");
    }

    #[test]
    fn parse_invalid_toml() {
        let result = parse_toml("this is not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn parse_json_format() {
        let json = r#"{
            "ontology": { "name": "test", "version": "1.0" },
            "nodes": [{ "label": "Node", "source": "table", "id_column": "id" }],
            "edges": []
        }"#;

        let ontology = parse_json(json).unwrap();
        assert_eq!(ontology.name(), "test");
        assert_eq!(ontology.node_labels(), vec!["Node"]);
    }
}
