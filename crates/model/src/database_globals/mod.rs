use common::{
    document::{
        ParseDocument,
        ParsedDocument,
    },
    query::{
        Order,
        Query,
    },
    runtime::Runtime,
};
use database::{
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use migrations_model::DatabaseVersion;
use value::{
    TableName,
    TableNamespace,
};

use self::types::{
    DatabaseGlobals,
    StorageTagInitializer,
    StorageType,
};
use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;

pub const DATABASE_GLOBALS_TABLE: TableName = TableName::const_new("_db");

pub struct DatabaseGlobalsTable;
impl SystemTable for DatabaseGlobalsTable {
    type Metadata = DatabaseGlobals;

    const TABLE_NAME: TableName = DATABASE_GLOBALS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct DatabaseGlobalsModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> DatabaseGlobalsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn database_globals(&mut self) -> anyhow::Result<ParsedDocument<DatabaseGlobals>> {
        let metadata_query = Query::full_table_scan(DATABASE_GLOBALS_TABLE.clone(), Order::Asc);
        let mut query_stream = ResolvedQuery::new(self.tx, TableNamespace::Global, metadata_query)?;
        let globals: ParsedDocument<DatabaseGlobals> =
            match query_stream.expect_at_most_one(self.tx).await? {
                Some(globals) => globals.parse()?,
                None => anyhow::bail!("Database globals were not found??"),
            };
        Ok(globals)
    }

    pub async fn replace_database_globals(
        &mut self,
        database_globals: ParsedDocument<DatabaseGlobals>,
    ) -> anyhow::Result<()> {
        SystemMetadataModel::new_global(self.tx)
            .replace(
                database_globals.id(),
                database_globals.into_value().try_into()?,
            )
            .await?;
        Ok(())
    }

    pub async fn initialize(&mut self, initial_version: DatabaseVersion) -> anyhow::Result<()> {
        let aws_prefix_secret = self.tx.runtime().new_uuid_v4().to_string();
        let globals = DatabaseGlobals {
            version: initial_version,
            aws_prefix_secret,
            storage_type: None,
        };
        SystemMetadataModel::new_global(self.tx)
            .insert(&DATABASE_GLOBALS_TABLE, globals.try_into()?)
            .await?;
        Ok(())
    }

    pub async fn initialize_storage_tag(
        &mut self,
        storage_tag: StorageTagInitializer,
        instance_name: String,
    ) -> anyhow::Result<StorageType> {
        // Read the storage tag out of the DB and use it to create storage.
        let mut database_globals = self.database_globals().await?;

        // Make sure we start up with a storage configuration that matches the
        // database's configuration. Fail loudly if things don't match.
        let storage_type = match (&storage_tag, &database_globals.storage_type) {
            (
                StorageTagInitializer::Local { dir },
                Some(storage_type @ StorageType::Local { dir: db_dir }),
            ) => {
                // Allow switching from different local directories. This isn't common
                // but could happen if the directory is moved and then the backend gets
                // restarted with the new location.
                if dir.to_string_lossy() != db_dir.as_str() {
                    let new_storage_type = StorageType::Local {
                        dir: dir.to_string_lossy().into(),
                    };
                    tracing::info!(
                        "Switching storage tag from local dir {} to {}",
                        db_dir,
                        dir.to_string_lossy()
                    );
                    database_globals.storage_type = Some(new_storage_type.clone());
                    self.replace_database_globals(database_globals).await?;
                    new_storage_type
                } else {
                    storage_type.clone()
                }
            },
            (StorageTagInitializer::S3, Some(storage_type @ StorageType::S3 { s3_prefix })) => {
                anyhow::ensure!(
                    s3_prefix.starts_with(&format!("{instance_name}-")),
                    "Cannot use s3 storage path {s3_prefix} with {instance_name}"
                );
                storage_type.clone()
            },
            // DB not initialized yet. Initialize it
            (_, None) => {
                let storage_type = match storage_tag {
                    StorageTagInitializer::S3 => {
                        let s3_prefix_secret = self.tx.runtime().new_uuid_v4().to_string();
                        let s3_prefix = format!("{instance_name}-{s3_prefix_secret}/");
                        StorageType::S3 { s3_prefix }
                    },
                    StorageTagInitializer::Local { dir } => StorageType::Local {
                        dir: dir.to_string_lossy().into(),
                    },
                };
                database_globals.storage_type = Some(storage_type.clone());
                self.replace_database_globals(database_globals).await?;
                storage_type
            },
            (storage_tag, db_storage_type) => anyhow::bail!(
                "Database was initialized with {db_storage_type:?}, but backend started up with \
                 {storage_tag:?}."
            ),
        };
        Ok(storage_type)
    }
}
