//! Ontology-driven graph registration.
//!
//! Connects the `fusiongraph-ontology` schema to the live query path: every
//! [`EdgeDefinition`] in an ontology is projected into a CSR graph through
//! the operator pipeline (table scan → `CoalescePartitionsExec` →
//! [`CSRBuilderExec`]) and registered in a [`GraphCatalog`] so it is
//! immediately queryable via `graph_traverse` in SQL.
//!
//! ```ignore
//! let ontology = Ontology::from_file("fusiongraph.toml")?;
//! let catalog = GraphCatalog::new();
//! register_graph_traverse(&ctx, &catalog);
//! let names = register_ontology_graphs(&ctx, &ontology, &catalog).await?;
//! // SELECT * FROM graph_traverse('iam_graph.CAN_ASSUME', 0, 3)
//! ```

use std::sync::Arc;

use arrow_schema::DataType;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::{cast, col};
use datafusion::physical_plan::coalesce_partitions::CoalescePartitionsExec;
use datafusion::physical_plan::collect;
use datafusion::prelude::SessionContext;

use fusiongraph_ontology::{EdgeDefinition, IdTransform, Ontology};

use crate::dictionary::NodeDictionary;
use crate::exec::{new_graph_sink, CSRBuilderExec, CsrBuildConfig};
use crate::udtf::GraphCatalog;

/// Returns the catalog name a projected edge graph is registered under:
/// `"<ontology_name>.<edge_label>"`, or just `"<edge_label>"` when the
/// ontology is unnamed.
#[must_use]
pub fn graph_name(ontology: &Ontology, edge: &EdgeDefinition) -> String {
    if ontology.name().is_empty() {
        edge.label.clone()
    } else {
        format!("{}.{}", ontology.name(), edge.label)
    }
}

/// Projects every edge definition in `ontology` into a CSR graph and
/// registers each in `catalog`.
///
/// For each [`EdgeDefinition`], the source table is resolved against `ctx`,
/// only the `from_column`/`to_column` pair is scanned (cast to `UInt64`, so
/// integer ID columns of any width work), and the result is streamed through
/// [`CSRBuilderExec`]. Graphs are registered under
/// the name produced by [`graph_name`] for each ontology/edge pair.
///
/// Returns the registered graph names in edge-definition order.
///
/// Supported [`EdgeDefinition`] features: `weight_column` (projected into a
/// weighted CSR, cast to `Float32`, NULLs default to 1.0) and temporal
/// validity columns (see [`register_ontology_graphs_as_of`]).
///
/// ## ID handling
///
/// Determined per edge from the referenced node definitions' `id_transform`
/// (and `settings.default_node_id_type`):
///
/// - `passthrough` (default): integer columns, cast to `UInt64`
/// - `extract_numeric`: digits extracted from string IDs
///   (e.g. `"user_42"` → 42) via `regexp_replace`, then cast
/// - `hash_u64` / `hash_u32` / `uuid_to_u128`, or
///   `default_node_id_type = "string"`: **dictionary encoding** — each
///   distinct key gets the next dense ID. For a dense CSR this subsumes
///   hashing (deterministic within a build, collision-free, reversible);
///   the graph is registered with a
///   [`NodeDictionary`], enabling string
///   start nodes in `graph_traverse` and key join-back via
///   `graph_nodes('name')`.
///
/// Composite `id_column` keys apply to node tables; [`EdgeDefinition`] has
/// single `from_column`/`to_column` fields, so composite *edge* keys are
/// not representable in the schema.
///
/// # Errors
///
/// Returns an error if ontology validation fails, a source table or column
/// is missing, an ID column cannot be cast to `UInt64`, or the CSR build
/// fails.
pub async fn register_ontology_graphs(
    ctx: &SessionContext,
    ontology: &Ontology,
    catalog: &Arc<GraphCatalog>,
) -> Result<Vec<String>> {
    register_ontology_graphs_as_of(ctx, ontology, catalog, None).await
}

/// [`register_ontology_graphs`] with temporal filtering.
///
/// When `as_of` is `Some`, edges whose definition declares
/// `valid_from_column` / `valid_to_column` are filtered to those valid at
/// the given instant: `valid_from <= as_of AND (valid_to IS NULL OR
/// valid_to > as_of)`. The literal must be comparable to the columns'
/// types under `DataFusion` coercion (e.g. an ISO-8601 string against
/// `Utf8` or `Timestamp` columns). Edges without temporal columns are
/// unaffected.
///
/// # Errors
///
/// See [`register_ontology_graphs`].
pub async fn register_ontology_graphs_as_of(
    ctx: &SessionContext,
    ontology: &Ontology,
    catalog: &Arc<GraphCatalog>,
    as_of: Option<&str>,
) -> Result<Vec<String>> {
    ontology
        .validate_or_error()
        .map_err(|e| DataFusionError::Plan(format!("ontology validation failed: {e}")))?;

    let mut registered = Vec::with_capacity(ontology.edges.len());

    for edge in &ontology.edges {
        let name = graph_name(ontology, edge);
        let context = |e| {
            DataFusionError::Context(
                format!(
                    "while projecting edge '{}' from table '{}'",
                    edge.label, edge.source
                ),
                Box::new(e),
            )
        };

        if needs_dictionary(ontology, edge) {
            let (graph, dictionary) = build_edge_graph_with_dictionary(ctx, edge, as_of)
                .await
                .map_err(context)?;
            catalog.register_with_dictionary(name.clone(), graph, dictionary);
        } else {
            let graph = build_edge_graph(ctx, ontology, edge, as_of)
                .await
                .map_err(context)?;
            catalog.register(name.clone(), graph);
        }
        registered.push(name);
    }

    Ok(registered)
}

/// Whether the edge's node references require dictionary encoding
/// (string-keyed IDs that cannot address a dense CSR directly).
fn needs_dictionary(ontology: &Ontology, edge: &EdgeDefinition) -> bool {
    if ontology.settings.default_node_id_type == fusiongraph_ontology::IdType::String {
        return true;
    }
    [&edge.from_node, &edge.to_node]
        .into_iter()
        .filter_map(|label| ontology.node(label))
        .any(|node| {
            matches!(
                node.id_transform,
                IdTransform::HashU64 | IdTransform::HashU32 | IdTransform::UuidToU128
            )
        })
}

/// Projection expression for one ID column on the numeric path, honoring
/// the referenced node's `id_transform`.
fn numeric_id_expr(
    ontology: &Ontology,
    node_label: &str,
    column: &str,
) -> datafusion::logical_expr::Expr {
    use datafusion::logical_expr::lit;
    let extract = ontology
        .node(node_label)
        .is_some_and(|n| n.id_transform == IdTransform::ExtractNumeric);
    let source = if extract {
        // Strip every non-digit character, then cast: "user_42" -> 42.
        datafusion::functions::regex::expr_fn::regexp_replace(
            col(column),
            lit("[^0-9]"),
            lit(""),
            Some(lit("g")),
        )
    } else {
        col(column)
    };
    cast(source, DataType::UInt64).alias(column)
}

/// Applies the temporal validity filter for `as_of`, when configured.
fn apply_temporal_filter(
    mut df: datafusion::dataframe::DataFrame,
    edge: &EdgeDefinition,
    as_of: Option<&str>,
) -> Result<datafusion::dataframe::DataFrame> {
    if let Some(instant) = as_of {
        use datafusion::logical_expr::lit;
        if let Some(from_col) = &edge.valid_from_column {
            df = df.filter(col(from_col.as_str()).lt_eq(lit(instant)))?;
        }
        if let Some(to_col) = &edge.valid_to_column {
            df = df.filter(
                col(to_col.as_str())
                    .is_null()
                    .or(col(to_col.as_str()).gt(lit(instant))),
            )?;
        }
    }
    Ok(df)
}

/// Builds a CSR graph for a single edge definition via the operator pipeline.
async fn build_edge_graph(
    ctx: &SessionContext,
    ontology: &Ontology,
    edge: &EdgeDefinition,
    as_of: Option<&str>,
) -> Result<Arc<fusiongraph_core::CsrGraph>> {
    let df = ctx.table(edge.source.as_str()).await?;
    let df = apply_temporal_filter(df, edge, as_of)?;

    // Selective projection: only the columns the kernel needs leave the
    // scan. Casting to UInt64 accepts the common lakehouse integer ID types
    // (Int32/Int64/UInt32/UInt64); invalid values surface as cast errors at
    // execution. Weights are cast to the CSR's Float32 storage.
    let mut projection = vec![
        numeric_id_expr(ontology, &edge.from_node, &edge.from_column),
        numeric_id_expr(ontology, &edge.to_node, &edge.to_column),
    ];
    if let Some(weight) = &edge.weight_column {
        projection.push(cast(col(weight.as_str()), DataType::Float32).alias(weight.as_str()));
    }
    let df = df.select(projection)?;

    let scan = df.create_physical_plan().await?;
    let coalesced = Arc::new(CoalescePartitionsExec::new(scan));

    let sink = new_graph_sink();
    let config = CsrBuildConfig {
        source_column: edge.from_column.clone(),
        target_column: edge.to_column.clone(),
        graph_sink: Some(Arc::clone(&sink)),
        weight_column: edge.weight_column.clone(),
        ..CsrBuildConfig::default()
    };
    let builder = Arc::new(CSRBuilderExec::new(coalesced, config));

    collect(builder, ctx.task_ctx()).await?;

    sink.get().map(Arc::clone).ok_or_else(|| {
        DataFusionError::Execution(
            "CSRBuilderExec completed without publishing a graph".to_string(),
        )
    })
}

/// Builds a CSR graph for a string-keyed edge definition, interning node
/// keys into a [`NodeDictionary`] (see the module docs on ID handling).
///
/// Dictionary projection collects the scan (not streaming yet): interning
/// requires observing every key before the CSR build anyway.
async fn build_edge_graph_with_dictionary(
    ctx: &SessionContext,
    edge: &EdgeDefinition,
    as_of: Option<&str>,
) -> Result<(Arc<fusiongraph_core::CsrGraph>, Arc<NodeDictionary>)> {
    use arrow_array::{Float32Array, StringArray};

    let df = ctx.table(edge.source.as_str()).await?;
    let df = apply_temporal_filter(df, edge, as_of)?;

    let mut projection = vec![
        cast(col(edge.from_column.as_str()), DataType::Utf8).alias(edge.from_column.as_str()),
        cast(col(edge.to_column.as_str()), DataType::Utf8).alias(edge.to_column.as_str()),
    ];
    let weighted = edge.weight_column.is_some();
    if let Some(weight) = &edge.weight_column {
        projection.push(cast(col(weight.as_str()), DataType::Float32).alias(weight.as_str()));
    }
    let df = df.select(projection)?;

    let mut dictionary = NodeDictionary::new();
    let mut edges: Vec<(u64, u64)> = Vec::new();
    let mut weights: Vec<f32> = Vec::new();

    for batch in df.collect().await? {
        let from = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                DataFusionError::Execution(format!(
                    "ID column '{}' did not cast to Utf8",
                    edge.from_column
                ))
            })?;
        let to = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                DataFusionError::Execution(format!(
                    "ID column '{}' did not cast to Utf8",
                    edge.to_column
                ))
            })?;
        let weight_col = if weighted {
            Some(
                batch
                    .column(2)
                    .as_any()
                    .downcast_ref::<Float32Array>()
                    .ok_or_else(|| {
                        DataFusionError::Execution(
                            "weight column did not cast to Float32".to_string(),
                        )
                    })?,
            )
        } else {
            None
        };

        for i in 0..batch.num_rows() {
            if arrow_array::Array::is_null(from, i) || arrow_array::Array::is_null(to, i) {
                continue;
            }
            let f = dictionary.get_or_insert(from.value(i));
            let t = dictionary.get_or_insert(to.value(i));
            edges.push((f, t));
            if let Some(w) = weight_col {
                weights.push(if arrow_array::Array::is_null(w, i) {
                    1.0
                } else {
                    w.value(i)
                });
            }
        }
    }

    let builder = fusiongraph_core::csr::CsrBuilder::new();
    let graph = if weighted {
        builder.with_weighted_edges(edges.into_iter().zip(weights).map(|((f, t), w)| (f, t, w)))
    } else {
        builder.with_edges(edges)
    }
    .build()
    .map_err(|e| DataFusionError::External(Box::new(e)))?;

    Ok((Arc::new(graph), Arc::new(dictionary)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udtf::register_graph_traverse;
    use arrow_array::{Int64Array, RecordBatch, UInt64Array};
    use arrow_schema::{Field, Schema};
    use datafusion::datasource::MemTable;

    const ONTOLOGY_TOML: &str = r#"
[ontology]
name = "iam_graph"
version = "1.0"

[[nodes]]
label = "Role"
source = "roles"
id_column = "role_id"

[[nodes]]
label = "Policy"
source = "policies"
id_column = "policy_id"

[[edges]]
label = "CAN_ASSUME"
source = "role_assumptions"
from_node = "Role"
from_column = "role_id"
to_node = "Role"
to_column = "assumed_role_id"

[[edges]]
label = "HAS_POLICY"
source = "role_policies"
from_node = "Role"
from_column = "role_id"
to_node = "Policy"
to_column = "policy_id"
"#;

    fn register_edge_table(
        ctx: &SessionContext,
        name: &str,
        from_name: &str,
        to_name: &str,
        edges: &[(i64, i64)],
    ) {
        // Int64 on purpose: exercises the cast-to-UInt64 path.
        let schema = Arc::new(Schema::new(vec![
            Field::new(from_name, DataType::Int64, false),
            Field::new(to_name, DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(
                    edges.iter().map(|&(f, _)| f).collect::<Vec<_>>(),
                )),
                Arc::new(Int64Array::from(
                    edges.iter().map(|&(_, t)| t).collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap();
        let table = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table(name, Arc::new(table)).unwrap();
    }

    fn setup() -> (SessionContext, Arc<GraphCatalog>, Ontology) {
        let ctx = SessionContext::new();
        // 0 -> 1 -> 2 role-assumption chain, plus role 0 grants policy 10.
        register_edge_table(
            &ctx,
            "role_assumptions",
            "role_id",
            "assumed_role_id",
            &[(0, 1), (1, 2)],
        );
        register_edge_table(&ctx, "role_policies", "role_id", "policy_id", &[(0, 10)]);

        let catalog = GraphCatalog::new();
        register_graph_traverse(&ctx, &catalog);
        let ontology = Ontology::from_toml(ONTOLOGY_TOML).unwrap();
        (ctx, catalog, ontology)
    }

    #[tokio::test]
    async fn registers_one_graph_per_edge_definition() {
        let (ctx, catalog, ontology) = setup();

        let names = register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        assert_eq!(
            names,
            vec![
                "iam_graph.CAN_ASSUME".to_string(),
                "iam_graph.HAS_POLICY".to_string()
            ]
        );
        assert_eq!(catalog.names(), names);
    }

    #[tokio::test]
    async fn registered_graphs_are_queryable_via_sql() {
        let (ctx, catalog, ontology) = setup();
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        let batches = ctx
            .sql(
                "SELECT node_id, depth \
                 FROM graph_traverse('iam_graph.CAN_ASSUME', 0, 2) \
                 ORDER BY node_id",
            )
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 3, "chain 0 -> 1 -> 2");
        let node_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(node_ids.values(), &[0, 1, 2]);
    }

    #[tokio::test]
    async fn missing_source_table_errors_with_edge_context() {
        let (ctx, catalog, mut ontology) = setup();
        ontology.edges[0].source = "nonexistent_table".to_string();

        let err = register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("CAN_ASSUME"), "got: {err}");
        assert!(err.contains("nonexistent_table"), "got: {err}");
    }

    #[tokio::test]
    async fn invalid_ontology_fails_before_any_build() {
        let (ctx, catalog, mut ontology) = setup();
        // Dangling edge: references an undefined node label.
        ontology.edges[0].from_node = "Ghost".to_string();

        let err = register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("ontology validation failed"), "got: {err}");
        assert!(catalog.names().is_empty(), "nothing should be registered");
    }

    #[test]
    fn unnamed_ontology_uses_bare_edge_label() {
        let mut ontology = Ontology::from_toml(ONTOLOGY_TOML).unwrap();
        ontology.ontology.name = String::new();
        assert_eq!(graph_name(&ontology, &ontology.edges[0]), "CAN_ASSUME");
    }

    #[tokio::test]
    async fn weighted_edges_project_into_weighted_csr() {
        use arrow_array::Float64Array;

        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("src", DataType::Int64, false),
            Field::new("dst", DataType::Int64, false),
            Field::new("score", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![0i64, 1])),
                Arc::new(Int64Array::from(vec![1i64, 2])),
                Arc::new(Float64Array::from(vec![2.5f64, 0.5])),
            ],
        )
        .unwrap();
        let table = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("scored_edges", Arc::new(table)).unwrap();

        let ontology = Ontology::from_toml(
            r#"
[ontology]
name = "w"

[[nodes]]
label = "N"
source = "scored_edges"
id_column = "src"

[[edges]]
label = "SCORED"
source = "scored_edges"
from_node = "N"
from_column = "src"
to_node = "N"
to_column = "dst"
weight_column = "score"
"#,
        )
        .unwrap();

        let catalog = GraphCatalog::new();
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        let graph = catalog.get("w.SCORED").unwrap();
        assert_eq!(graph.edge_count(), 2);
        assert!(
            graph.shards().iter().any(|s| s.has_weights()),
            "CSR should carry weights"
        );
        // Weight of edge 0 -> 1 (first edge of node 0) survives projection.
        let (shard_idx, offset) = graph
            .global_to_shard(fusiongraph_core::NodeId::new(0))
            .unwrap();
        let shard = &graph.shards()[shard_idx];
        let (start, _) = shard.neighbor_range(offset);
        assert_eq!(shard.weight(start), Some(2.5));
    }

    #[tokio::test]
    async fn temporal_as_of_filters_edges() {
        use arrow_array::StringArray;

        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("src", DataType::Int64, false),
            Field::new("dst", DataType::Int64, false),
            Field::new("valid_from", DataType::Utf8, false),
            Field::new("valid_to", DataType::Utf8, true),
        ]));
        // Edge 0->1 valid all of 2025; edge 1->2 valid from 2026 onwards.
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![0i64, 1])),
                Arc::new(Int64Array::from(vec![1i64, 2])),
                Arc::new(StringArray::from(vec!["2025-01-01", "2026-01-01"])),
                Arc::new(StringArray::from(vec![Some("2026-01-01"), None])),
            ],
        )
        .unwrap();
        let table = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("grants", Arc::new(table)).unwrap();

        let ontology = Ontology::from_toml(
            r#"
[ontology]
name = "t"

[[nodes]]
label = "N"
source = "grants"
id_column = "src"

[[edges]]
label = "GRANTS"
source = "grants"
from_node = "N"
from_column = "src"
to_node = "N"
to_column = "dst"
valid_from_column = "valid_from"
valid_to_column = "valid_to"
"#,
        )
        .unwrap();

        // As of mid-2025: only 0 -> 1 is valid.
        let catalog = GraphCatalog::new();
        register_ontology_graphs_as_of(&ctx, &ontology, &catalog, Some("2025-06-15"))
            .await
            .unwrap();
        assert_eq!(catalog.get("t.GRANTS").unwrap().edge_count(), 1);

        // As of mid-2026: only 1 -> 2 (0 -> 1 expired).
        let catalog2 = GraphCatalog::new();
        register_ontology_graphs_as_of(&ctx, &ontology, &catalog2, Some("2026-06-15"))
            .await
            .unwrap();
        let g = catalog2.get("t.GRANTS").unwrap();
        assert_eq!(g.edge_count(), 1);
        assert!(g.has_edge(
            fusiongraph_core::NodeId::new(1),
            fusiongraph_core::NodeId::new(2)
        ));

        // Without as_of: all edges included.
        let catalog3 = GraphCatalog::new();
        register_ontology_graphs(&ctx, &ontology, &catalog3)
            .await
            .unwrap();
        assert_eq!(catalog3.get("t.GRANTS").unwrap().edge_count(), 2);
    }

    fn string_edge_ctx() -> SessionContext {
        use arrow_array::StringArray;
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("follower", DataType::Utf8, false),
            Field::new("followee", DataType::Utf8, false),
        ]));
        // alice -> bob -> carol, alice -> carol.
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(StringArray::from(vec!["alice", "bob", "alice"])),
                Arc::new(StringArray::from(vec!["bob", "carol", "carol"])),
            ],
        )
        .unwrap();
        ctx.register_table(
            "follows",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap()),
        )
        .unwrap();
        ctx
    }

    const STRING_ONTOLOGY: &str = r#"
[ontology]
name = "social"

[settings]
default_node_id_type = "string"

[[nodes]]
label = "User"
source = "follows"
id_column = "follower"

[[edges]]
label = "FOLLOWS"
source = "follows"
from_node = "User"
from_column = "follower"
to_node = "User"
to_column = "followee"
"#;

    #[tokio::test]
    async fn string_keyed_ontology_builds_dictionary_graph() {
        let ctx = string_edge_ctx();
        let ontology = Ontology::from_toml(STRING_ONTOLOGY).unwrap();
        let catalog = GraphCatalog::new();
        register_graph_traverse(&ctx, &catalog);

        let names = register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();
        assert_eq!(names, vec!["social.FOLLOWS".to_string()]);

        let dict = catalog.dictionary("social.FOLLOWS").expect("dictionary");
        assert_eq!(dict.len(), 3);
        assert_eq!(dict.id_of("alice"), Some(0));
        assert_eq!(dict.key_of(2), Some("carol"));

        // Traverse by string start node, join keys back via graph_nodes.
        let batches = ctx
            .sql(
                "SELECT k.node_key, t.depth \
                 FROM graph_traverse('social.FOLLOWS', 'alice', 3) t \
                 JOIN graph_nodes('social.FOLLOWS') k ON k.node_id = t.node_id \
                 ORDER BY t.depth, k.node_key",
            )
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 3);
        let keys = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(keys.value(0), "alice");
        // bob and carol both reachable; carol at depth 1 via direct edge.
        assert_eq!(keys.value(1), "bob");
        assert_eq!(keys.value(2), "carol");
    }

    #[tokio::test]
    async fn unknown_string_start_node_errors() {
        let ctx = string_edge_ctx();
        let ontology = Ontology::from_toml(STRING_ONTOLOGY).unwrap();
        let catalog = GraphCatalog::new();
        register_graph_traverse(&ctx, &catalog);
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('social.FOLLOWS', 'mallory', 3)")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("'mallory' not found"), "got: {err}");
    }

    #[tokio::test]
    async fn string_start_without_dictionary_errors() {
        let (ctx, catalog, ontology) = setup(); // integer-keyed graphs
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        let err = ctx
            .sql("SELECT * FROM graph_traverse('iam_graph.CAN_ASSUME', 'alice', 3)")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no node-key dictionary"), "got: {err}");
    }

    #[tokio::test]
    async fn extract_numeric_transform_parses_prefixed_ids() {
        use arrow_array::StringArray;
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("src", DataType::Utf8, false),
            Field::new("dst", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(StringArray::from(vec!["user_0", "user_1"])),
                Arc::new(StringArray::from(vec!["user_1", "user_2"])),
            ],
        )
        .unwrap();
        ctx.register_table(
            "prefixed",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap()),
        )
        .unwrap();

        let ontology = Ontology::from_toml(
            r#"
[ontology]
name = "p"

[[nodes]]
label = "U"
source = "prefixed"
id_column = "src"
id_transform = "extract_numeric"

[[edges]]
label = "E"
source = "prefixed"
from_node = "U"
from_column = "src"
to_node = "U"
to_column = "dst"
"#,
        )
        .unwrap();

        let catalog = GraphCatalog::new();
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        let graph = catalog.get("p.E").unwrap();
        assert!(
            catalog.dictionary("p.E").is_none(),
            "extract_numeric is a numeric path, no dictionary"
        );
        // "user_0" -> 0, "user_1" -> 1, "user_2" -> 2.
        assert!(graph.has_edge(
            fusiongraph_core::NodeId::new(0),
            fusiongraph_core::NodeId::new(1)
        ));
        assert!(graph.has_edge(
            fusiongraph_core::NodeId::new(1),
            fusiongraph_core::NodeId::new(2)
        ));
    }

    #[tokio::test]
    async fn dictionary_survives_compaction_swap() {
        use fusiongraph_core::types::EdgeData;
        use fusiongraph_core::CompactionPolicy;

        let ctx = string_edge_ctx();
        let ontology = Ontology::from_toml(STRING_ONTOLOGY).unwrap();
        let catalog = GraphCatalog::new();
        register_ontology_graphs(&ctx, &ontology, &catalog)
            .await
            .unwrap();

        // Mutate past a strict policy, compact, and verify the dictionary
        // still resolves keys against the swapped entry.
        let graph = catalog.get("social.FOLLOWS").unwrap();
        graph.delta().insert(
            fusiongraph_core::NodeId::new(2),
            fusiongraph_core::NodeId::new(0),
            EdgeData::default(),
        );
        let strict = CompactionPolicy {
            max_delta_entries: 1,
            max_delta_ratio: 10.0,
        };
        assert!(catalog
            .compact_if_needed("social.FOLLOWS", &strict)
            .unwrap());

        let dict = catalog
            .dictionary("social.FOLLOWS")
            .expect("dictionary preserved across compaction");
        assert_eq!(dict.id_of("alice"), Some(0));
        let compacted = catalog.get("social.FOLLOWS").unwrap();
        assert!(compacted.has_edge(
            fusiongraph_core::NodeId::new(2),
            fusiongraph_core::NodeId::new(0)
        ));
    }
}
