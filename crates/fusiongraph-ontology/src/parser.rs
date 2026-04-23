//! Ontology parser for TOML and JSON formats.

use std::path::Path;

use crate::error::{OntologyError, Result};
use crate::schema::Ontology;

/// Parses an ontology from a TOML string.
///
/// # Errors
///
/// Returns [`OntologyError::ParseError`] when the content is not valid TOML.
pub fn parse_toml(content: &str) -> Result<Ontology> {
    toml::from_str(content).map_err(|e| OntologyError::ParseError {
        message: e.to_string(),
    })
}

/// Parses an ontology from a JSON string.
///
/// # Errors
///
/// Returns [`OntologyError::JsonError`] when the content is not valid JSON.
pub fn parse_json(content: &str) -> Result<Ontology> {
    serde_json::from_str(content).map_err(|e| OntologyError::JsonError {
        message: e.to_string(),
    })
}

/// Parses an ontology from a file (auto-detects format).
///
/// # Errors
///
/// Returns IO errors when the file cannot be read, or parse errors when the
/// file contents are neither valid TOML nor valid JSON.
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
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::ParseError`] when the content is not valid TOML.
    pub fn from_toml(content: &str) -> Result<Self> {
        parse_toml(content)
    }

    /// Parses from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::JsonError`] when the content is not valid JSON.
    pub fn from_json(content: &str) -> Result<Self> {
        parse_json(content)
    }

    /// Loads from a file.
    ///
    /// # Errors
    ///
    /// Returns IO errors when the file cannot be read, or parse errors when the
    /// file contents are neither valid TOML nor valid JSON.
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

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_identifier() -> impl Strategy<Value = String> {
        "[a-zA-Z][a-zA-Z0-9_]{0,20}".prop_map(|s| s)
    }

    fn arb_node_def() -> impl Strategy<Value = crate::schema::NodeDefinition> {
        (arb_identifier(), arb_identifier(), arb_identifier()).prop_map(
            |(label, source, id_column)| crate::schema::NodeDefinition {
                label,
                source,
                id_column: crate::schema::IdColumn::Single(id_column),
                id_transform: crate::schema::IdTransform::Passthrough,
                properties: vec![],
                filter: None,
            },
        )
    }

    fn arb_ontology() -> impl Strategy<Value = crate::schema::Ontology> {
        (
            arb_identifier(),
            prop::collection::vec(arb_node_def(), 0..5),
        )
            .prop_map(|(name, nodes)| crate::schema::Ontology {
                ontology: crate::schema::OntologyMeta {
                    name,
                    version: "1.0".to_string(),
                    description: String::new(),
                },
                settings: Default::default(),
                nodes,
                edges: vec![],
                properties: vec![],
            })
    }

    proptest! {
        #[test]
        fn toml_roundtrip(ontology in arb_ontology()) {
            let toml_str = toml::to_string(&ontology).unwrap();
            let parsed = parse_toml(&toml_str).unwrap();
            prop_assert_eq!(ontology.name(), parsed.name());
            prop_assert_eq!(ontology.node_labels().len(), parsed.node_labels().len());
        }

        #[test]
        fn json_roundtrip(ontology in arb_ontology()) {
            let json_str = serde_json::to_string(&ontology).unwrap();
            let parsed = parse_json(&json_str).unwrap();
            prop_assert_eq!(ontology.name(), parsed.name());
            prop_assert_eq!(ontology.node_labels().len(), parsed.node_labels().len());
        }

        #[test]
        fn toml_json_equivalence(ontology in arb_ontology()) {
            let toml_str = toml::to_string(&ontology).unwrap();
            let json_str = serde_json::to_string(&ontology).unwrap();
            let from_toml = parse_toml(&toml_str).unwrap();
            let from_json = parse_json(&json_str).unwrap();
            prop_assert_eq!(from_toml.name(), from_json.name());
            prop_assert_eq!(from_toml.node_labels(), from_json.node_labels());
        }

        #[test]
        fn invalid_toml_does_not_panic(s in ".{0,256}") {
            let _ = parse_toml(&s);
        }

        #[test]
        fn invalid_json_does_not_panic(s in ".{0,256}") {
            let _ = parse_json(&s);
        }
    }
}
