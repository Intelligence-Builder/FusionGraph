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

use fusiongraph_ontology::{EdgeDefinition, Ontology};

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
/// Current limitations (documented, enforced by clear errors where possible):
/// - `weight_column` is ignored (CSR build is unweighted today)
/// - temporal validity columns are ignored
/// - string/UUID ID transforms are not applied; IDs must be integers
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
    ontology
        .validate_or_error()
        .map_err(|e| DataFusionError::Plan(format!("ontology validation failed: {e}")))?;

    let mut registered = Vec::with_capacity(ontology.edges.len());

    for edge in &ontology.edges {
        let graph = build_edge_graph(ctx, edge).await.map_err(|e| {
            DataFusionError::Context(
                format!(
                    "while projecting edge '{}' from table '{}'",
                    edge.label, edge.source
                ),
                Box::new(e),
            )
        })?;

        let name = graph_name(ontology, edge);
        catalog.register(name.clone(), graph);
        registered.push(name);
    }

    Ok(registered)
}

/// Builds a CSR graph for a single edge definition via the operator pipeline.
async fn build_edge_graph(
    ctx: &SessionContext,
    edge: &EdgeDefinition,
) -> Result<Arc<fusiongraph_core::CsrGraph>> {
    // Selective projection: only the two ID columns leave the scan. Casting
    // to UInt64 accepts the common lakehouse integer ID types (Int32/Int64/
    // UInt32/UInt64); invalid values surface as cast errors at execution.
    let df = ctx.table(edge.source.as_str()).await?.select(vec![
        cast(col(edge.from_column.as_str()), DataType::UInt64).alias(edge.from_column.as_str()),
        cast(col(edge.to_column.as_str()), DataType::UInt64).alias(edge.to_column.as_str()),
    ])?;

    let scan = df.create_physical_plan().await?;
    let coalesced = Arc::new(CoalescePartitionsExec::new(scan));

    let sink = new_graph_sink();
    let config = CsrBuildConfig {
        source_column: edge.from_column.clone(),
        target_column: edge.to_column.clone(),
        graph_sink: Some(Arc::clone(&sink)),
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
}
