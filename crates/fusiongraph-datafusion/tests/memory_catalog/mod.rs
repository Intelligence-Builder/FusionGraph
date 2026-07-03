//! Minimal in-memory Iceberg catalog for integration tests.
//!
//! `iceberg-catalog-memory` has no release compatible with `iceberg` 0.5.1
//! (all published versions are yanked), and its 0.5.1-era implementation did
//! not support `update_table`, which `Transaction::commit` requires. This
//! test-only catalog implements the subset `FusionGraph`'s tests need —
//! namespace/table creation, load, and commit (metadata update) — adapted
//! from the Apache iceberg-rust memory catalog (Apache License 2.0).
//!
//! Not intended for production use.

use std::collections::HashMap;

use async_trait::async_trait;
use iceberg::io::FileIO;
use iceberg::spec::{TableMetadata, TableMetadataBuilder};
use iceberg::table::Table;
use iceberg::{
    Catalog, Error, ErrorKind, Namespace, NamespaceIdent, Result, TableCommit, TableCreation,
    TableIdent,
};
use tokio::sync::Mutex;

/// Test-only in-memory Iceberg catalog backed by local-filesystem `FileIO`.
#[derive(Debug)]
pub struct TestMemoryCatalog {
    file_io: FileIO,
    warehouse: String,
    namespaces: Mutex<Vec<NamespaceIdent>>,
    /// Table identifier -> current metadata file location.
    tables: Mutex<HashMap<TableIdent, String>>,
}

impl TestMemoryCatalog {
    /// Creates a catalog writing metadata/data under `warehouse`.
    pub fn new(file_io: FileIO, warehouse: String) -> Self {
        Self {
            file_io,
            warehouse,
            namespaces: Mutex::new(Vec::new()),
            tables: Mutex::new(HashMap::new()),
        }
    }

    async fn write_metadata(&self, metadata: &TableMetadata) -> Result<String> {
        let location = format!(
            "{}/metadata/{}-{}.metadata.json",
            metadata.location(),
            metadata.last_sequence_number(),
            uuid::Uuid::new_v4()
        );
        self.file_io
            .new_output(&location)?
            .write(serde_json::to_vec(metadata)?.into())
            .await?;
        Ok(location)
    }

    async fn table_from_location(
        &self,
        ident: TableIdent,
        metadata_location: String,
    ) -> Result<Table> {
        let content = self.file_io.new_input(&metadata_location)?.read().await?;
        let metadata = serde_json::from_slice::<TableMetadata>(&content)?;
        Table::builder()
            .file_io(self.file_io.clone())
            .metadata_location(metadata_location)
            .metadata(metadata)
            .identifier(ident)
            .build()
    }
}

fn unsupported(what: &str) -> Error {
    Error::new(
        ErrorKind::FeatureUnsupported,
        format!("TestMemoryCatalog does not support {what}"),
    )
}

#[async_trait]
impl Catalog for TestMemoryCatalog {
    async fn list_namespaces(
        &self,
        _parent: Option<&NamespaceIdent>,
    ) -> Result<Vec<NamespaceIdent>> {
        Ok(self.namespaces.lock().await.clone())
    }

    async fn create_namespace(
        &self,
        namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> Result<Namespace> {
        self.namespaces.lock().await.push(namespace.clone());
        Ok(Namespace::new(namespace.clone()))
    }

    async fn get_namespace(&self, namespace: &NamespaceIdent) -> Result<Namespace> {
        if self.namespace_exists(namespace).await? {
            Ok(Namespace::new(namespace.clone()))
        } else {
            Err(Error::new(
                ErrorKind::Unexpected,
                format!("namespace {namespace:?} does not exist"),
            ))
        }
    }

    async fn namespace_exists(&self, namespace: &NamespaceIdent) -> Result<bool> {
        Ok(self.namespaces.lock().await.contains(namespace))
    }

    async fn update_namespace(
        &self,
        _namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> Result<()> {
        Err(unsupported("update_namespace"))
    }

    async fn drop_namespace(&self, namespace: &NamespaceIdent) -> Result<()> {
        self.namespaces.lock().await.retain(|n| n != namespace);
        Ok(())
    }

    async fn list_tables(&self, namespace: &NamespaceIdent) -> Result<Vec<TableIdent>> {
        Ok(self
            .tables
            .lock()
            .await
            .keys()
            .filter(|t| t.namespace() == namespace)
            .cloned()
            .collect())
    }

    async fn create_table(
        &self,
        namespace: &NamespaceIdent,
        creation: TableCreation,
    ) -> Result<Table> {
        let ident = TableIdent::new(namespace.clone(), creation.name.clone());

        let creation = if creation.location.is_some() {
            creation
        } else {
            TableCreation {
                location: Some(format!(
                    "{}/{}/{}",
                    self.warehouse,
                    namespace.join("/"),
                    ident.name()
                )),
                ..creation
            }
        };

        let metadata = TableMetadataBuilder::from_table_creation(creation)?
            .build()?
            .metadata;
        let metadata_location = self.write_metadata(&metadata).await?;

        self.tables
            .lock()
            .await
            .insert(ident.clone(), metadata_location.clone());

        Table::builder()
            .file_io(self.file_io.clone())
            .metadata_location(metadata_location)
            .metadata(metadata)
            .identifier(ident)
            .build()
    }

    async fn load_table(&self, table: &TableIdent) -> Result<Table> {
        let location = self
            .tables
            .lock()
            .await
            .get(table)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unexpected,
                    format!("table {table:?} does not exist"),
                )
            })?;
        self.table_from_location(table.clone(), location).await
    }

    async fn drop_table(&self, table: &TableIdent) -> Result<()> {
        self.tables.lock().await.remove(table);
        Ok(())
    }

    async fn table_exists(&self, table: &TableIdent) -> Result<bool> {
        Ok(self.tables.lock().await.contains_key(table))
    }

    async fn rename_table(&self, _src: &TableIdent, _dest: &TableIdent) -> Result<()> {
        Err(unsupported("rename_table"))
    }

    /// Applies a commit: checks requirements, applies updates to the current
    /// metadata, writes the new metadata file, and swaps the pointer.
    async fn update_table(&self, mut commit: TableCommit) -> Result<Table> {
        let ident = commit.identifier().clone();
        let current = self.load_table(&ident).await?;

        for requirement in commit.take_requirements() {
            requirement.check(Some(current.metadata()))?;
        }

        let mut builder = TableMetadataBuilder::new_from_metadata(
            current.metadata().clone(),
            current.metadata_location().map(ToString::to_string),
        );
        for update in commit.take_updates() {
            builder = update.apply(builder)?;
        }
        let metadata = builder.build()?.metadata;

        let metadata_location = self.write_metadata(&metadata).await?;
        self.tables
            .lock()
            .await
            .insert(ident.clone(), metadata_location.clone());

        Table::builder()
            .file_io(self.file_io.clone())
            .metadata_location(metadata_location)
            .metadata(metadata)
            .identifier(ident)
            .build()
    }
}
