//! Apache Iceberg integration (feature: `iceberg`, enabled by default).
//!
//! Registers Iceberg tables as `DataFusion` tables via the official
//! [`iceberg-datafusion`] provider, which prunes data files using Iceberg
//! manifest statistics when filters are pushed down. Once registered, an
//! Iceberg edge table is a regular table in the session, so the whole
//! `FusionGraph` pipeline works on top of it unchanged:
//!
//! - [`register_ontology_graphs`](crate::register_ontology_graphs) projects
//!   Iceberg edge tables into CSR graphs, and
//! - `graph_traverse` makes them queryable from SQL.
//!
//! Snapshot pinning ([`register_iceberg_table_snapshot`]) gives reproducible
//! graph builds: the projected graph corresponds to an exact Iceberg snapshot
//! ID, regardless of concurrent appends to the table.
//!
//! [`iceberg-datafusion`]: https://crates.io/crates/iceberg-datafusion

use std::sync::Arc;

use datafusion::error::{DataFusionError, Result};
use datafusion::prelude::SessionContext;
use iceberg::table::Table;
use iceberg_datafusion::IcebergTableProvider;

fn external(e: iceberg::Error) -> DataFusionError {
    DataFusionError::External(Box::new(e))
}

/// Registers an Iceberg table under `name` in the session, reading from the
/// table's **current** snapshot.
///
/// # Errors
///
/// Returns an error if the provider cannot be constructed (e.g. the table
/// metadata is unreadable) or if `name` is already registered.
pub async fn register_iceberg_table(ctx: &SessionContext, name: &str, table: Table) -> Result<()> {
    let provider = IcebergTableProvider::try_new_from_table(table)
        .await
        .map_err(external)?;
    ctx.register_table(name, Arc::new(provider))?;
    Ok(())
}

/// Registers an Iceberg table under `name`, **pinned to a specific snapshot**.
///
/// Graphs projected from a pinned table are reproducible: re-running the
/// build yields the same topology even while writers append new snapshots.
/// Record the snapshot ID next to the graph name to make graph builds
/// auditable.
///
/// # Errors
///
/// Returns an error if the snapshot does not exist in the table metadata,
/// the provider cannot be constructed, or `name` is already registered.
pub async fn register_iceberg_table_snapshot(
    ctx: &SessionContext,
    name: &str,
    table: Table,
    snapshot_id: i64,
) -> Result<()> {
    let provider = IcebergTableProvider::try_new_from_table_snapshot(table, snapshot_id)
        .await
        .map_err(external)?;
    ctx.register_table(name, Arc::new(provider))?;
    Ok(())
}
