#![feature(coroutines)]
#![feature(binary_heap_pop_if)]
#![feature(iter_advance_by)]
#![feature(type_alias_impl_trait)]
#![feature(iterator_try_collect)]
#![feature(never_type)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
#![feature(impl_trait_in_assoc_type)]
#![feature(cow_is_borrowed)]
#![feature(try_find)]
#![feature(once_cell_try)]

mod bootstrap_model;
mod committer;
mod database;
mod database_index_workers;
mod execution_size;
mod file_uploads;
mod metrics;
pub mod patch;
pub mod persistence_helpers;
mod preloaded;
pub mod query;
pub mod reads;
mod retention;
mod search_flusher_wake;
mod search_index_bootstrap;
mod snapshot_manager;
mod stack_traces;
pub mod streaming_export_selection;
pub mod subscription;
pub mod system_query;
pub mod system_tables;
mod table_registry;
pub mod table_summary;
mod table_usage;
mod token;
mod transaction;
mod transaction_id_generator;
mod transaction_index;
mod virtual_tables;
mod write_batcher;
mod write_limits;
mod write_log;
mod write_throughput_limiter;
mod writes;

mod component_registry;
mod schema_registry;
pub use committer::{
    table_dependency_sort_key,
    AFTER_PENDING_WRITE_SNAPSHOT,
};
pub use component_registry::ComponentRegistry;
pub use database_index_workers::{
    index_writer::{
        IndexRateLimit,
        IndexWriter,
        PERFORM_BACKFILL_LABEL,
    },
    IndexWorker,
};
pub use execution_size::{
    FileStorageReadSize,
    FileStorageWriteSize,
    FunctionExecutionSize,
    TransactionLimits,
};
pub use file_uploads::{
    PendingFileUpload,
    UploadedFileUpload,
};
pub use indexing::database_index_snapshot::{
    DatabaseIndexSnapshotCache,
    TimestampedIndexCache,
};
pub use patch::PatchValue;
pub use preloaded::PreloadedIndexRange;
pub use reads::{
    ReadSet,
    TransactionReadSet,
    TransactionReadSize,
    OVER_LIMIT_HELP,
};
pub use schema_registry::SchemaRegistry;
pub use search_flusher_wake::{
    SearchFlusherWakeSignals,
    SearchFlusherWakeSubscriber,
};
pub use search_index_bootstrap::FINISHED_BOOTSTRAP_UPDATES;
pub use table_iteration::{
    data_sync::{
        DataSyncCursor,
        DataSyncIterator,
        DataSyncStatus,
    },
    LatestTableIterator,
    MultiTableIterator,
    TableIterator,
    TableScanCursor,
};
pub use table_registry::TableRegistry;
pub use token::{
    SerializedToken,
    Token,
};
pub use transaction::{
    SnoopedTransaction,
    TableCountSnapshot,
    Transaction,
};
pub use transaction_index::{
    SearchNotEnabled,
    TextIndexManagerSnapshot,
    TransactionTextSnapshot,
};
pub use virtual_tables::VirtualTable;
pub use write_limits::BiggestDocumentWrites;
pub use write_log::{
    LogReader,
    WriteSource,
};
pub use writes::{
    TransactionWriteSize,
    Writes,
};

pub use self::{
    bootstrap_model::{
        components::{
            definition::{
                ComponentDefinitionsTable,
                COMPONENT_DEFINITIONS_TABLE,
            },
            BootstrapComponentsModel,
            ComponentsTable,
            COMPONENTS_BY_PARENT_INDEX,
            COMPONENTS_TABLE,
        },
        defaults,
        import_facing::ImportFacingModel,
        index::{
            IndexModel,
            IndexTable,
        },
        index_backfills::{
            types::IndexBackfillMetadata,
            IndexBackfillModel,
            IndexBackfillTable,
            INDEX_BACKFILLS_BY_INDEX_ID,
            INDEX_BACKFILLS_TABLE,
        },
        index_workers::{
            load_metadata_fast_forward_ts,
            IndexWorkerMetadataModel,
            IndexWorkerMetadataRecord,
            IndexWorkerMetadataTable,
            INDEX_DOC_ID_INDEX,
            INDEX_WORKER_METADATA_TABLE,
        },
        next_persistence_index_id::{
            types::NextPersistenceIndexIdMetadata,
            NextPersistenceIndexIdTable,
            NEXT_PERSISTENCE_INDEX_ID_TABLE,
        },
        schema::{
            types::{
                SchemaDiff,
                SerializedSchemaDiff,
            },
            SchemaModel,
            SchemasTable,
            SCHEMAS_STATE_INDEX,
            SCHEMAS_TABLE,
            SCHEMA_STATE_FIELD,
        },
        schema_validation_progress::{
            types::SchemaValidationProgress,
            SchemaValidationProgressModel,
            SchemaValidationProgressTable,
            SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID,
            SCHEMA_VALIDATION_PROGRESS_TABLE,
        },
        schema_validations::{
            types::{
                SchemaValidationMetadata,
                ValidationState,
            },
            SchemaValidationModel,
            SchemaValidationTable,
            SchemaValidationWithProgress,
            StagedValidationWithProgress,
            ValidationAttemptUpdate,
            SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME,
            SCHEMA_VALIDATIONS_TABLE,
        },
        system_metadata::SystemMetadataModel,
        table::{
            TableModel,
            TablesTable,
            NUM_RESERVED_LEGACY_TABLE_NUMBERS,
            NUM_RESERVED_SYSTEM_TABLE_NUMBERS,
            TABLES_BY_NAME_INDEX,
        },
        user_facing::UserFacingModel,
    },
    database::{
        unauthorized_error,
        BootstrapMetadata,
        Database,
        DatabaseSnapshot,
        DocumentDeltas,
        OccRetryStats,
        SnapshotPage,
        MAX_OCC_FAILURES,
    },
    database_index_workers::index_writer::IndexSelector,
    query::{
        soft_data_limit,
        DeveloperQuery,
        ResolvedQuery,
    },
    retention::{
        latest_retention_min_snapshot_ts,
        FollowerRetentionManager,
        IndexRetentionSource,
        LeaderRetentionManager,
        LeaderRetentionWorkers,
        RetentionType,
    },
    snapshot_manager::{
        Snapshot,
        TableCounts,
    },
    subscription::{
        InvalidationEvent,
        Subscription,
    },
    table_summary::{
        TableCount,
        TableShape,
        TableShapes,
        TableSummaries,
        TableSummary,
        TableSummaryWriter,
    },
    table_usage::{
        TableUsage,
        TablesUsage,
    },
    transaction::DEFAULT_PAGE_SIZE,
    transaction_id_generator::TransactionIdGenerator,
    transaction_index::TransactionIndex,
};
pub use crate::metrics::shutdown_error;
