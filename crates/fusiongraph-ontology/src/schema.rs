//! Ontology schema types.

use serde::{Deserialize, Serialize};

/// Complete ontology definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ontology {
    /// Ontology metadata.
    #[serde(default)]
    pub ontology: OntologyMeta,
    /// Global settings.
    #[serde(default)]
    pub settings: OntologySettings,
    /// Node definitions.
    #[serde(default)]
    pub nodes: Vec<NodeDefinition>,
    /// Edge definitions.
    #[serde(default)]
    pub edges: Vec<EdgeDefinition>,
    /// Computed properties.
    #[serde(default)]
    pub properties: Vec<ComputedProperty>,
}

impl Ontology {
    /// Returns the ontology name.
    pub fn name(&self) -> &str {
        &self.ontology.name
    }

    /// Returns the ontology version.
    pub fn version(&self) -> &str {
        &self.ontology.version
    }

    /// Returns node labels.
    pub fn node_labels(&self) -> Vec<&str> {
        self.nodes.iter().map(|n| n.label.as_str()).collect()
    }

    /// Returns edge labels.
    pub fn edge_labels(&self) -> Vec<&str> {
        self.edges.iter().map(|e| e.label.as_str()).collect()
    }

    /// Finds a node definition by label.
    pub fn node(&self, label: &str) -> Option<&NodeDefinition> {
        self.nodes.iter().find(|n| n.label == label)
    }

    /// Finds an edge definition by label.
    pub fn edge(&self, label: &str) -> Option<&EdgeDefinition> {
        self.edges.iter().find(|e| e.label == label)
    }
}

/// Ontology metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OntologyMeta {
    /// Ontology name.
    #[serde(default)]
    pub name: String,
    /// Ontology version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Description.
    #[serde(default)]
    pub description: String,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// Global ontology settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologySettings {
    /// Default ID type for nodes.
    #[serde(default)]
    pub default_node_id_type: IdType,
    /// Edge direction mode.
    #[serde(default)]
    pub edge_direction: EdgeDirection,
    /// Allow self-loops.
    #[serde(default)]
    pub allow_self_loops: bool,
    /// Allow parallel edges.
    #[serde(default = "default_true")]
    pub allow_parallel_edges: bool,
}

impl Default for OntologySettings {
    fn default() -> Self {
        Self {
            default_node_id_type: IdType::U64,
            edge_direction: EdgeDirection::Directed,
            allow_self_loops: false,
            allow_parallel_edges: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Node ID type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdType {
    /// 32-bit unsigned integer.
    U32,
    /// 64-bit unsigned integer.
    #[default]
    U64,
    /// 128-bit unsigned integer (for UUIDs).
    U128,
    /// String IDs (will be hashed).
    String,
}

/// Edge direction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeDirection {
    /// Edges have a direction.
    #[default]
    Directed,
    /// Edges are bidirectional.
    Undirected,
}

/// Node definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefinition {
    /// Node label (e.g., "User", "Account").
    pub label: String,
    /// Source table (fully qualified).
    pub source: String,
    /// ID column(s).
    pub id_column: IdColumn,
    /// ID transformation.
    #[serde(default)]
    pub id_transform: IdTransform,
    /// Properties to include.
    #[serde(default)]
    pub properties: Vec<String>,
    /// Optional filter predicate (SQL).
    #[serde(default)]
    pub filter: Option<String>,
}

/// ID column specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdColumn {
    /// Single column ID.
    Single(String),
    /// Composite key.
    Composite {
        /// Column names.
        columns: Vec<String>,
        /// Separator for concatenation.
        #[serde(default = "default_separator")]
        separator: String,
    },
}

fn default_separator() -> String {
    "::".to_string()
}

impl IdColumn {
    /// Returns the column names.
    pub fn columns(&self) -> Vec<&str> {
        match self {
            Self::Single(col) => vec![col.as_str()],
            Self::Composite { columns, .. } => columns.iter().map(String::as_str).collect(),
        }
    }
}

/// ID transformation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdTransform {
    /// Pass through numeric IDs unchanged.
    #[default]
    Passthrough,
    /// Hash string to u64 (FNV-1a).
    HashU64,
    /// Hash string to u32 (FNV-1a).
    HashU32,
    /// Convert UUID to u128.
    UuidToU128,
    /// Extract numeric portion from string.
    ExtractNumeric,
}

/// Edge definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDefinition {
    /// Edge label (e.g., "BELONGS_TO", "CAN_ASSUME").
    pub label: String,
    /// Source table for edge data.
    pub source: String,
    /// Source node label.
    pub from_node: String,
    /// Column containing source node ID.
    pub from_column: String,
    /// Target node label.
    pub to_node: String,
    /// Column containing target node ID.
    pub to_column: String,
    /// Edge properties to include.
    #[serde(default)]
    pub properties: Vec<String>,
    /// Weight column (for weighted graphs).
    #[serde(default)]
    pub weight_column: Option<String>,
    /// Default weight if column is NULL.
    #[serde(default = "default_weight")]
    pub weight_default: f64,
    /// Whether edge is implicit (derived from node table).
    #[serde(default)]
    pub implicit: bool,
    /// Skip edges with NULL targets.
    #[serde(default = "default_true")]
    pub skip_null_targets: bool,
    /// Temporal: valid-from column.
    #[serde(default)]
    pub valid_from_column: Option<String>,
    /// Temporal: valid-to column.
    #[serde(default)]
    pub valid_to_column: Option<String>,
    /// Partition column hint.
    #[serde(default)]
    pub partition_column: Option<String>,
}

fn default_weight() -> f64 {
    1.0
}

/// Computed property definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedProperty {
    /// Property name.
    pub name: String,
    /// Node label (mutually exclusive with edge).
    #[serde(default)]
    pub node: Option<String>,
    /// Edge label (mutually exclusive with node).
    #[serde(default)]
    pub edge: Option<String>,
    /// SQL expression to compute.
    pub expression: String,
    /// Whether to compute at build time.
    #[serde(default)]
    pub materialized: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_column_single() {
        let col = IdColumn::Single("user_id".to_string());
        assert_eq!(col.columns(), vec!["user_id"]);
    }

    #[test]
    fn id_column_composite() {
        let col = IdColumn::Composite {
            columns: vec!["account_id".to_string(), "region".to_string()],
            separator: "::".to_string(),
        };
        assert_eq!(col.columns(), vec!["account_id", "region"]);
    }

    #[test]
    fn default_settings() {
        let settings = OntologySettings::default();
        assert_eq!(settings.default_node_id_type, IdType::U64);
        assert_eq!(settings.edge_direction, EdgeDirection::Directed);
        assert!(!settings.allow_self_loops);
        assert!(settings.allow_parallel_edges);
    }
}
