//! Authoritative metadata in our system is stored in tables with prefix
//! [`METADATA_PREFIX`]. Each file in this module stores a category of system
//! metadata.
//!
//! Users are not allowed to create table names that start with
//! [`METADATA_PREFIX`]. This choice is similar to to the `_id` field
//! automatically inserted into documents, as the user is not allowed to mutate
//! those either.
//!
//! The core design principle here is that all authoritative system metadata is
//! stored as a document and can be read by UDFs and subscribed to, just like
//! any other document. However, we do *not* allow the user to mutate these
//! through the general purpose `insert`, `update`, `replace` and `delete` APIs,
//! since they have stronger invariants than regular documents. Instead, we
//! provide special purpose APIs for the restricted modifications we'd like to
//! allow. Linux's `procfs` is an inspiration here, where it's useful to present
//! system data as regular files, but most mutations don't make much sense.

#![feature(coroutines)]
#![feature(iter_advance_by)]
#![feature(type_alias_impl_trait)]
#![feature(iterator_try_collect)]
#![feature(never_type)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
#![feature(impl_trait_in_assoc_type)]
#![feature(iter_from_coroutine)]
#![feature(duration_constructors)]

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::LazyLock,
};

use audit_log_config::{
    AuditLogConfigTable,
    AUDIT_LOG_CONFIG_TABLE,
};
use auth::AUTH_TABLE;
use aws_lambda_versions::{
    AwsLambdaVersionsTable,
    AWS_LAMBDA_VERSIONS_TABLE,
};
use backend_info::{
    BackendInfoTable,
    BACKEND_INFO_TABLE,
};
use backend_state::{
    BackendStateTable,
    BACKEND_STATE_TABLE,
};
use canonical_urls::CANONICAL_URLS_TABLE;
use common::{
    bootstrap_model::{
        index::{
            IndexConfig,
            IndexMetadata,
            INDEX_TABLE,
        },
        tables::TABLES_TABLE,
    },
    document::CREATION_TIME_FIELD_PATH,
    runtime::Runtime,
    types::{
        IndexName,
        TabletIndexName,
    },
    virtual_system_mapping::{
        AssociatedVirtualTable,
        VirtualSystemMapping,
    },
};
use components::handles::{
    FunctionHandlesTable,
    BY_COMPONENT_PATH_INDEX,
    FUNCTION_HANDLES_TABLE,
};
use cron_jobs::{
    CRON_JOBS_INDEX_BY_NAME,
    CRON_JOBS_TABLE,
    CRON_JOB_LOGS_INDEX_BY_NAME_TS,
    CRON_JOB_LOGS_TABLE,
    CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID,
    CRON_NEXT_RUN_INDEX_BY_NEXT_TS,
    CRON_NEXT_RUN_TABLE,
};
pub use database::system_tables::{
    SystemIndex,
    SystemTable,
};
use database::{
    defaults::bootstrap_system_tables,
    system_tables::ErasedSystemTable,
    BootstrapComponentsModel,
    ComponentDefinitionsTable,
    ComponentsTable,
    Database,
    IndexBackfillTable,
    IndexModel,
    IndexTable,
    IndexWorkerMetadataTable,
    NextPersistenceIndexIdTable,
    SchemaValidationProgressTable,
    SchemaValidationTable,
    SchemasTable,
    TablesTable,
    Transaction,
    COMPONENTS_BY_PARENT_INDEX,
    COMPONENTS_TABLE,
    COMPONENT_DEFINITIONS_TABLE,
    INDEX_BACKFILLS_BY_INDEX_ID,
    INDEX_BACKFILLS_TABLE,
    INDEX_DOC_ID_INDEX,
    INDEX_WORKER_METADATA_TABLE,
    NEXT_PERSISTENCE_INDEX_ID_TABLE,
    NUM_RESERVED_LEGACY_TABLE_NUMBERS,
    SCHEMAS_STATE_INDEX,
    SCHEMAS_TABLE,
    SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME,
    SCHEMA_VALIDATIONS_TABLE,
    SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID,
    SCHEMA_VALIDATION_PROGRESS_TABLE,
    TABLES_BY_NAME_INDEX,
};
use database_globals::{
    DatabaseGlobalsModel,
    DatabaseGlobalsTable,
    DATABASE_GLOBALS_TABLE,
};
use environment_variables::{
    ENVIRONMENT_VARIABLES_INDEX_BY_NAME,
    ENVIRONMENT_VARIABLES_TABLE,
};
use exports::{
    EXPORTS_BY_REQUESTOR,
    EXPORTS_BY_STATE_AND_TS_INDEX,
    EXPORTS_TABLE,
};
use external_packages::ExternalPackagesTable;
use file_storage::{
    FileStorageTable,
    FILE_STORAGE_ID_INDEX,
    FILE_STORAGE_TABLE,
};
use keybroker::Identity;
use log_sinks::LogSinksTable;
use maplit::{
    btreemap,
    btreeset,
};
use migrations_model::{
    DatabaseVersion,
    DATABASE_VERSION,
};
use modules::{
    ModulesTable,
    MODULES_TABLE,
    MODULE_INDEX_BY_PATH,
};
use scheduled_jobs::{
    ScheduledJobsTable,
    SCHEDULED_JOBS_INDEX,
    SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS,
    SCHEDULED_JOBS_INDEX_BY_UDF_PATH,
    SCHEDULED_JOBS_TABLE,
};
use session_requests::{
    SessionRequestsTable,
    SESSION_REQUESTS_INDEX,
    SESSION_REQUESTS_TABLE,
};
use snapshot_imports::{
    SnapshotImportsTable,
    SNAPSHOT_IMPORTS_TABLE,
};
use source_packages::{
    SourcePackagesTable,
    SOURCE_PACKAGES_TABLE,
};
use strum::IntoEnumIterator;
use udf_config::{
    UdfConfigTable,
    UDF_CONFIG_TABLE,
};
pub use value::METADATA_PREFIX;
use value::{
    TableName,
    TableNamespace,
    TableNumber,
};

use crate::{
    auth::AuthTable,
    backend_state::BackendStateModel,
    canonical_urls::CanonicalUrlsTable,
    cron_jobs::{
        CronJobLogsTable,
        CronJobsTable,
        CronNextRunTable,
    },
    data_sync_progress::{
        DataSyncProgressTable,
        DATA_SYNC_PROGRESS_INDEX_BY_LAST_UPDATED,
        DATA_SYNC_PROGRESS_INDEX_BY_SYNC_ID,
        DATA_SYNC_PROGRESS_TABLE,
    },
    deployment_audit_log::{
        DeploymentAuditLogsTable,
        AUDIT_LOG_INDEX_BY_ACTION,
        DEPLOYMENT_AUDIT_LOG_TABLE,
    },
    environment_variables::EnvironmentVariablesTable,
    exports::ExportsTable,
    external_packages::EXTERNAL_PACKAGES_TABLE,
    log_sinks::LOG_SINKS_TABLE,
    s3_export_state::{
        S3ExportStateTable,
        S3_EXPORT_STATE_BY_CONFIG_ID,
        S3_EXPORT_STATE_TABLE,
    },
    scheduled_jobs::args::{
        ScheduledJobArgsTable,
        SCHEDULED_JOBS_ARGS_TABLE,
    },
    usage_limits::{
        UsageLimitsTable,
        USAGE_LIMITS_INDEX_BY_SELECTOR,
        USAGE_LIMITS_TABLE,
    },
};

pub mod airbyte_import;
pub mod audit_log_config;
pub mod auth;
pub mod aws_lambda_versions;
pub mod backend_info;
pub mod backend_state;
pub mod canonical_urls;
pub mod components;
pub mod config;
pub mod cron_jobs;
pub mod data_sync_progress;
pub mod database_globals;
pub mod deployment_audit_log;
pub mod environment_variables;
pub mod exports;
pub mod external_packages;
pub mod file_storage;
pub mod fivetran_import;
pub mod log_sinks;
mod metrics;
pub mod migrations;
pub mod modules;
pub mod s3_export_state;
pub mod scheduled_jobs;
pub mod session_requests;
pub mod snapshot_imports;
pub mod source_packages;
pub mod udf_config;
pub mod usage_limits;

/// Default best effort table number when creating the table. If it is taken
/// already, another number is selected. Legacy deployments don't
/// respect this at all. Consistency is mainly nice for making snapshot
/// import/export more likely to work nicely.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, strum::EnumIter)]
enum DefaultTableNumber {
    Tables = 1,
    Index = 2,
    Exports = 4,
    BackendInfo = 5,
    UdfConfig = 6,
    Auth = 7,
    DatabaseGlobals = 8,
    Modules = 9,
    SourcePackages = 12,
    EnvironmentVariables = 13,
    AwsLambdaVersions = 14,
    DeploymentAuditLogs = 15,
    SessionRequests = 17,
    CronJobs = 19,
    Schemas = 20,
    CronJobLogs = 21,
    LogSinks = 23,
    BackendState = 24,
    ExternalPackages = 25,
    ScheduledJobs = 27,
    FileStorage = 28,
    SnapshotImports = 29,
    IndexWorkerMetadata = 30,
    ComponentDefinitionsTable = 31,
    ComponentsTable = 32,
    FunctionHandlesTable = 33,
    CanonicalUrls = 34,
    CronNextRun = 35,
    IndexBackfills = 36,
    SchemaValidationProgress = 37,
    ScheduledJobArgs = 38,
    AuditLogConfig = 39,
    UsageLimits = 40,
    DataSyncProgress = 41,
    NextPersistenceIndexId = 42,
    SchemaValidations = 43,
    S3ExportState = 44,
    // Keep this number and your user name up to date. The number makes it easy to know
    // what to use next. The username on the same line detects merge conflicts
    // Next Number - 45 - nipunn
}

impl From<DefaultTableNumber> for TableNumber {
    fn from(value: DefaultTableNumber) -> Self {
        (NUM_RESERVED_LEGACY_TABLE_NUMBERS + value as u32)
            .try_into()
            .unwrap()
    }
}

impl From<DefaultTableNumber> for &'static dyn ErasedSystemTable {
    fn from(value: DefaultTableNumber) -> Self {
        match value {
            DefaultTableNumber::Tables => &TablesTable,
            DefaultTableNumber::Index => &IndexTable,
            DefaultTableNumber::Exports => &ExportsTable,
            DefaultTableNumber::BackendInfo => &BackendInfoTable,
            DefaultTableNumber::UdfConfig => &UdfConfigTable,
            DefaultTableNumber::Auth => &AuthTable,
            DefaultTableNumber::DatabaseGlobals => &DatabaseGlobalsTable,
            DefaultTableNumber::Modules => &ModulesTable,
            DefaultTableNumber::SourcePackages => &SourcePackagesTable,
            DefaultTableNumber::EnvironmentVariables => &EnvironmentVariablesTable,
            DefaultTableNumber::AwsLambdaVersions => &AwsLambdaVersionsTable,
            DefaultTableNumber::DeploymentAuditLogs => &DeploymentAuditLogsTable,
            DefaultTableNumber::SessionRequests => &SessionRequestsTable,
            DefaultTableNumber::CronJobs => &CronJobsTable,
            DefaultTableNumber::Schemas => &SchemasTable,
            DefaultTableNumber::CronJobLogs => &CronJobLogsTable,
            DefaultTableNumber::LogSinks => &LogSinksTable,
            DefaultTableNumber::S3ExportState => &S3ExportStateTable,
            DefaultTableNumber::BackendState => &BackendStateTable,
            DefaultTableNumber::ExternalPackages => &ExternalPackagesTable,
            DefaultTableNumber::ScheduledJobs => &ScheduledJobsTable,
            DefaultTableNumber::FileStorage => &FileStorageTable,
            DefaultTableNumber::SnapshotImports => &SnapshotImportsTable,
            DefaultTableNumber::IndexWorkerMetadata => &IndexWorkerMetadataTable,
            DefaultTableNumber::ComponentDefinitionsTable => &ComponentDefinitionsTable,
            DefaultTableNumber::ComponentsTable => &ComponentsTable,
            DefaultTableNumber::FunctionHandlesTable => &FunctionHandlesTable,
            DefaultTableNumber::CanonicalUrls => &CanonicalUrlsTable,
            DefaultTableNumber::CronNextRun => &CronNextRunTable,
            DefaultTableNumber::IndexBackfills => &IndexBackfillTable,
            DefaultTableNumber::SchemaValidationProgress => &SchemaValidationProgressTable,
            DefaultTableNumber::SchemaValidations => &SchemaValidationTable,
            DefaultTableNumber::ScheduledJobArgs => &ScheduledJobArgsTable,
            DefaultTableNumber::AuditLogConfig => &AuditLogConfigTable,
            DefaultTableNumber::UsageLimits => &UsageLimitsTable,
            DefaultTableNumber::DataSyncProgress => &DataSyncProgressTable,
            DefaultTableNumber::NextPersistenceIndexId => &NextPersistenceIndexIdTable,
        }
    }
}

pub static DEFAULT_TABLE_NUMBERS: LazyLock<BTreeMap<TableName, TableNumber>> =
    LazyLock::new(|| {
        let mut default_table_numbers = BTreeMap::new();
        for default_table_number in DefaultTableNumber::iter() {
            let system_table: &'static dyn ErasedSystemTable = default_table_number.into();
            default_table_numbers.insert(
                system_table.table_name().clone(),
                default_table_number.into(),
            );
            if let Some(associated_virtual_table) = system_table.virtual_table() {
                match associated_virtual_table {
                    AssociatedVirtualTable::Primary {
                        virtual_table_name, ..
                    } => {
                        default_table_numbers
                            .insert(virtual_table_name, default_table_number.into());
                    },
                    // Secondary system tables don't share table numbers with virtual tables.
                    AssociatedVirtualTable::Secondary(_) => {},
                }
            }
        }
        default_table_numbers
    });

/// System indexes all end with creation time. Except for these ones which
/// are too large and not worth to backfill.
///
/// New indexes should add creation time as a final tiebreak field.
static SYSTEM_INDEXES_WITHOUT_CREATION_TIME: LazyLock<BTreeSet<IndexName>> = LazyLock::new(|| {
    btreeset! {
        BY_COMPONENT_PATH_INDEX.name(),
        CRON_JOBS_INDEX_BY_NAME.name(),
        CRON_JOB_LOGS_INDEX_BY_NAME_TS.name(),
        CRON_NEXT_RUN_INDEX_BY_NEXT_TS.name(),
        CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID.name(),
        ENVIRONMENT_VARIABLES_INDEX_BY_NAME.name(),
        EXPORTS_BY_STATE_AND_TS_INDEX.name(),
        FILE_STORAGE_ID_INDEX.name(),
        MODULE_INDEX_BY_PATH.name(),
        SCHEDULED_JOBS_INDEX.name(),
        SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS.name(),
        SCHEDULED_JOBS_INDEX_BY_UDF_PATH.name(),
        SESSION_REQUESTS_INDEX.name(),
        TABLES_BY_NAME_INDEX.name(),
        SCHEMAS_STATE_INDEX.name(),
        INDEX_DOC_ID_INDEX.name(),
        COMPONENTS_BY_PARENT_INDEX.name(),
    }
});

/// Idempotently initialize all the tables.
pub async fn initialize_application_system_tables<RT: Runtime>(
    database: &Database<RT>,
) -> anyhow::Result<()> {
    let mut tx = database.begin(Identity::system()).await?;
    NextPersistenceIndexIdTable::initialize(
        &mut tx,
        DEFAULT_TABLE_NUMBERS
            .get(&NEXT_PERSISTENCE_INDEX_ID_TABLE)
            .copied(),
    )
    .await?;
    for table in app_system_tables() {
        if table.table_name() == NEXT_PERSISTENCE_INDEX_ID_TABLE {
            continue;
        }
        let is_new = initialize_application_system_table(
            &mut tx,
            table,
            TableNamespace::Global,
            &DEFAULT_TABLE_NUMBERS,
        )
        .await?;

        if is_new {
            // This is a bit ugly to put here for initialization, but it's a bit more
            // ergonomic this way instead of having initialize have <RT> generics
            if table.table_name() == BackendStateTable.table_name() {
                BackendStateModel::new(&mut tx).initialize().await?;
            }
            if table.table_name() == DatabaseGlobalsTable.table_name() {
                // This is a bit ugly to put here for initialization, but it's a bit more
                // ergonomic this way instead of having initialize have <RT> generics
                DatabaseGlobalsModel::new(&mut tx)
                    .initialize(DATABASE_VERSION)
                    .await?;
            }
        }
    }
    for component_id in BootstrapComponentsModel::new(&mut tx)
        .all_component_paths()
        .keys()
        .copied()
    {
        if component_id.is_root() {
            continue;
        }
        for table in component_system_tables().into_iter().chain([
            &SchemasTable as &dyn ErasedSystemTable,
            &SchemaValidationProgressTable,
            &SchemaValidationTable,
        ]) {
            initialize_application_system_table(
                &mut tx,
                table,
                component_id.into(),
                &DEFAULT_TABLE_NUMBERS,
            )
            .await?;
        }
    }

    database
        .commit_with_write_source(tx, "init_app_system_tables")
        .await?;

    // We could load indexes in memory asynchronously in order to speed up backend
    // start up time at the expense of going to the database while indexes load.
    // However, we opt-in to block here in order to smooth out the database load
    // during mass backend restarts and promotions.
    database
        .load_indexes_into_memory(APP_TABLES_TO_LOAD_IN_MEMORY.clone())
        .await?;

    Ok(())
}

pub async fn initialize_application_system_table<RT: Runtime>(
    tx: &mut Transaction<RT>,
    table: &dyn ErasedSystemTable,
    namespace: TableNamespace,
    default_table_numbers: &BTreeMap<TableName, TableNumber>,
) -> anyhow::Result<bool> {
    let is_new = tx
        .create_system_table(
            namespace,
            &table.table_name(),
            default_table_numbers.get(&table.table_name()).cloned(),
        )
        .await?;
    if is_new {
        for index in table.indexes() {
            let index_metadata = IndexMetadata::new_enabled(index.name, index.fields);
            IndexModel::new(tx)
                .add_system_index(namespace, index_metadata)
                .await?;
        }
    } else {
        let table_id = tx
            .table_mapping()
            .namespace(namespace)
            .id(&table.table_name())?
            .tablet_id;
        let existing_indexes: BTreeMap<_, _> = IndexModel::new(tx)
            .all_indexes_on_table(table_id)
            .await?
            .into_iter()
            .filter(|index| !index.name.is_by_id_or_creation_time())
            .map(|index| {
                let IndexConfig::Database {
                    spec,
                    on_disk_state: _,
                    persistence_index_id: _,
                } = &index.config
                else {
                    // This isn't a strict requirement; it's just not implemented or needed.
                    anyhow::bail!("system tables indexes must be Database");
                };
                anyhow::Ok((index.name.clone(), spec.fields.clone()))
            })
            .try_collect()?;

        // Create new indexes as backfilling.
        let defined_indexes = table.indexes();
        for index in defined_indexes.iter() {
            if !SYSTEM_INDEXES_WITHOUT_CREATION_TIME.contains(&index.name) {
                anyhow::ensure!(
                    index.fields.last() == Some(&*CREATION_TIME_FIELD_PATH),
                    "System index {} should end with _creationTime",
                    index.name
                );
            } else {
                anyhow::ensure!(
                    index.fields.last() != Some(&*CREATION_TIME_FIELD_PATH),
                    "System index {} correctly ends with _creationTime. Doesn't need to be in \
                     SYSTEM_INDEXES_WITHOUT_CREATION_TIME list.",
                    index.name
                );
            }
            let index_name = TabletIndexName::new(table_id, index.name.descriptor().clone())?;
            match existing_indexes.get(&index_name) {
                Some(existing_fields) => anyhow::ensure!(
                    existing_fields == &index.fields,
                    "{} has the wrong fields: {existing_fields} != {}",
                    index.name,
                    index.fields,
                ),
                None => {
                    let index_metadata = IndexMetadata::new_backfilling(
                        *tx.begin_timestamp(),
                        index.name.clone(),
                        index.fields.clone(),
                    );
                    IndexModel::new(tx)
                        .add_system_index(namespace, index_metadata)
                        .await?;
                },
            }
        }

        // Remove indexes that are no longer referenced
        for (index, _) in existing_indexes {
            let index_name =
                IndexName::new(table.table_name().clone(), index.descriptor().clone())?;
            if !defined_indexes
                .iter()
                .any(|defined_index| defined_index.name == index_name)
            {
                // Existing index is not referenced any more.
                IndexModel::new(tx)
                    .drop_system_index(namespace, index_name)
                    .await?;
            }
        }
    }

    Ok(is_new)
}

pub fn app_system_tables() -> Vec<&'static dyn ErasedSystemTable> {
    let mut system_tables: Vec<&'static dyn ErasedSystemTable> = vec![
        &DatabaseGlobalsTable,
        &DeploymentAuditLogsTable,
        &EnvironmentVariablesTable,
        &AuthTable,
        &ExternalPackagesTable,
        &SessionRequestsTable,
        &BackendStateTable,
        &ExportsTable,
        &SnapshotImportsTable,
        &FunctionHandlesTable,
        &CanonicalUrlsTable,
        &LogSinksTable,
        &S3ExportStateTable,
        &AuditLogConfigTable,
        &AwsLambdaVersionsTable,
        &BackendInfoTable,
        &UsageLimitsTable,
        &DataSyncProgressTable,
    ];
    system_tables.extend(component_system_tables());
    system_tables.extend(bootstrap_system_tables());
    system_tables
}

/// Schema tables are bootstrapped separately for each component.
pub fn component_system_tables() -> Vec<&'static dyn ErasedSystemTable> {
    vec![
        &FileStorageTable,
        &ScheduledJobsTable,
        &ScheduledJobArgsTable,
        &CronJobsTable,
        &CronJobLogsTable,
        &CronNextRunTable,
        &ModulesTable,
        &UdfConfigTable,
        &SourcePackagesTable,
    ]
}

static APP_TABLES_TO_LOAD_IN_MEMORY: LazyLock<BTreeSet<TableName>> = LazyLock::new(|| {
    btreeset! {
        UDF_CONFIG_TABLE.clone(),
        AUTH_TABLE.clone(),
        DATABASE_GLOBALS_TABLE.clone(),
        MODULES_TABLE.clone(),
        ENVIRONMENT_VARIABLES_TABLE.clone(),
        CRON_JOBS_TABLE.clone(),
        CRON_NEXT_RUN_TABLE.clone(),
        BACKEND_STATE_TABLE.clone(),
        CANONICAL_URLS_TABLE.clone(),
        BACKEND_INFO_TABLE.clone(),
        AWS_LAMBDA_VERSIONS_TABLE.clone(),
        SOURCE_PACKAGES_TABLE.clone(),
        USAGE_LIMITS_TABLE.clone(),
        NEXT_PERSISTENCE_INDEX_ID_TABLE.clone(),
    }
});

pub fn virtual_system_mapping() -> &'static VirtualSystemMapping {
    static MAPPING: LazyLock<VirtualSystemMapping> = LazyLock::new(|| {
        let mut mapping = VirtualSystemMapping::default();
        for table in app_system_tables() {
            if let Some(associated_virtual_table) = table.virtual_table() {
                mapping.add_table(table.table_name().clone(), associated_virtual_table)
            }
        }
        mapping
    });
    &MAPPING
}

pub static FIRST_SEEN_TABLE: LazyLock<BTreeMap<TableName, DatabaseVersion>> = LazyLock::new(|| {
    btreemap! {
        MODULES_TABLE.clone() => 44,
        DATABASE_GLOBALS_TABLE.clone() => 44,
        EXTERNAL_PACKAGES_TABLE.clone() => 77,
        DEPLOYMENT_AUDIT_LOG_TABLE.clone() => 44,
        SCHEDULED_JOBS_TABLE.clone() => 44,
        SESSION_REQUESTS_TABLE.clone() => 44,
        AUTH_TABLE.clone() => 44,
        FILE_STORAGE_TABLE.clone() => 44,
        UDF_CONFIG_TABLE.clone() => 44,
        BACKEND_STATE_TABLE.clone() => 75,
        CRON_JOBS_TABLE.clone() => 47,
        CRON_JOB_LOGS_TABLE.clone() => 51,
        CRON_NEXT_RUN_TABLE.clone() => 118,
        SOURCE_PACKAGES_TABLE.clone() => 44,
        ENVIRONMENT_VARIABLES_TABLE.clone() => 44,
        AWS_LAMBDA_VERSIONS_TABLE.clone() => 44,
        BACKEND_INFO_TABLE.clone() => 44,
        EXPORTS_TABLE.clone() => 44,
        LOG_SINKS_TABLE.clone() => 68,
        S3_EXPORT_STATE_TABLE.clone() => 134,
        TABLES_TABLE.clone() => 44,
        INDEX_TABLE.clone() => 44,
        SCHEMAS_TABLE.clone() => 50,
        SNAPSHOT_IMPORTS_TABLE.clone() => 89,
        INDEX_WORKER_METADATA_TABLE.clone() => 92,
        COMPONENTS_TABLE.clone() => 100,
        COMPONENT_DEFINITIONS_TABLE.clone() => 100,
        FUNCTION_HANDLES_TABLE.clone() => 102,
        CANONICAL_URLS_TABLE.clone() => 116,
        INDEX_BACKFILLS_TABLE.clone() => 120,
        SCHEMA_VALIDATION_PROGRESS_TABLE.clone() => 122,
        SCHEMA_VALIDATIONS_TABLE.clone() => 131,
        SCHEDULED_JOBS_ARGS_TABLE.clone() => 123,
        AUDIT_LOG_CONFIG_TABLE.clone() => 124,
        USAGE_LIMITS_TABLE.clone() => 126,
        DATA_SYNC_PROGRESS_TABLE.clone() => 127,
        NEXT_PERSISTENCE_INDEX_ID_TABLE.clone() => 129,
    }
});

pub static FIRST_SEEN_INDEX: LazyLock<BTreeMap<IndexName, DatabaseVersion>> = LazyLock::new(|| {
    btreemap! {
        MODULE_INDEX_BY_PATH.name() => 44,
        SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS.name() => 74,
        SCHEDULED_JOBS_INDEX.name() => 45,
        SCHEDULED_JOBS_INDEX_BY_UDF_PATH.name() => 44,
        SESSION_REQUESTS_INDEX.name() => 44,
        FILE_STORAGE_ID_INDEX.name() => 44,
        CRON_JOBS_INDEX_BY_NAME.name() => 49,
        CRON_JOB_LOGS_INDEX_BY_NAME_TS.name() => 51,
        CRON_NEXT_RUN_INDEX_BY_NEXT_TS.name() => 118,
        CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID.name() => 118,
        EXPORTS_BY_STATE_AND_TS_INDEX.name() => 88,
        TABLES_BY_NAME_INDEX.name() => 44,
        SCHEMAS_STATE_INDEX.name() => 44,
        ENVIRONMENT_VARIABLES_INDEX_BY_NAME.name() => 91,
        INDEX_DOC_ID_INDEX.name() => 92,
        COMPONENTS_BY_PARENT_INDEX.name() => 100,
        BY_COMPONENT_PATH_INDEX.name() => 102,
        EXPORTS_BY_REQUESTOR.name() => 110,
        INDEX_BACKFILLS_BY_INDEX_ID.name() => 120,
        SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID.name() => 132,
        SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME.name() => 131,
        USAGE_LIMITS_INDEX_BY_SELECTOR.name() => 126,
        DATA_SYNC_PROGRESS_INDEX_BY_SYNC_ID.name() => 127,
        S3_EXPORT_STATE_BY_CONFIG_ID.name() => 134,
        DATA_SYNC_PROGRESS_INDEX_BY_LAST_UPDATED.name() => 127,
        AUDIT_LOG_INDEX_BY_ACTION.name() => 128,
    }
});
