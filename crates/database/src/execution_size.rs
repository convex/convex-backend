use common::knobs::{
    TRANSACTION_MAX_FILE_READ_SIZE_BYTES,
    TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES,
    TRANSACTION_MAX_NUM_FILES_READ,
    TRANSACTION_MAX_NUM_FILES_WRITTEN,
    TRANSACTION_MAX_NUM_SCHEDULED,
    TRANSACTION_MAX_NUM_USER_WRITES,
    TRANSACTION_MAX_READ_SET_INTERVALS,
    TRANSACTION_MAX_READ_SIZE_BYTES,
    TRANSACTION_MAX_READ_SIZE_ROWS,
    TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES,
    TRANSACTION_MAX_USER_WRITE_SIZE_BYTES,
};
use serde::Deserialize;

use crate::{
    TransactionReadSize,
    TransactionWriteSize,
};

/// Metrics related to a function execution.
pub struct FunctionExecutionSize {
    pub num_intervals: usize,
    pub read_size: TransactionReadSize,
    pub write_size: TransactionWriteSize,
    pub scheduled_size: ScheduledFunctionsSize,
    pub file_storage_write_size: FileStorageWriteSize,
    pub file_storage_read_size: FileStorageReadSize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScheduledFunctionsSize {
    /// Number of scheduled functions
    pub num_writes: usize,
    /// Sum of scheduled argument sizes
    pub size: usize,
    /// Max of scheduled argument sizes
    pub max_args_size: usize,
}

/// Files written to file storage from within a transaction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileStorageWriteSize {
    /// Number of files written to file storage
    pub num_writes: usize,
    /// Sum of the sizes of the files written to file storage
    pub size: usize,
}

/// Files read from file storage from within a transaction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileStorageReadSize {
    /// Number of files read from file storage
    pub num_reads: usize,
    /// Sum of the sizes of the files read from file storage
    pub size: usize,
}

/// Transaction limits. All fields are resolved absolute values.
/// When deserialized from user input, missing fields fall back to global
/// defaults via `Default`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TransactionLimits {
    pub bytes_read: usize,
    pub documents_read: usize,
    pub database_queries: usize,
    pub documents_written: usize,
    pub bytes_written: usize,
    pub functions_scheduled: usize,
    pub scheduled_function_args_bytes: usize,
    pub files_written: usize,
    pub file_write_bytes: usize,
    pub files_read: usize,
    pub file_read_bytes: usize,
}

impl Default for TransactionLimits {
    fn default() -> Self {
        Self {
            bytes_read: *TRANSACTION_MAX_READ_SIZE_BYTES,
            documents_read: *TRANSACTION_MAX_READ_SIZE_ROWS,
            database_queries: *TRANSACTION_MAX_READ_SET_INTERVALS,
            documents_written: *TRANSACTION_MAX_NUM_USER_WRITES,
            bytes_written: *TRANSACTION_MAX_USER_WRITE_SIZE_BYTES,
            functions_scheduled: *TRANSACTION_MAX_NUM_SCHEDULED,
            scheduled_function_args_bytes: *TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES,
            files_written: *TRANSACTION_MAX_NUM_FILES_WRITTEN,
            file_write_bytes: *TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES,
            files_read: *TRANSACTION_MAX_NUM_FILES_READ,
            file_read_bytes: *TRANSACTION_MAX_FILE_READ_SIZE_BYTES,
        }
    }
}

impl TransactionLimits {
    /// Resolve a per-call `budget` (a delta on top of the transaction's
    /// current `usage`) into an absolute ceiling, clamped to the existing
    /// `ceiling`. Each dimension allows up to `usage + budget` cumulative
    /// use, never exceeding `ceiling`. Saturates on overflow.
    pub fn from_budget(budget: Self, usage: &FunctionExecutionSize, ceiling: &Self) -> Self {
        let combine = |used: usize, delta: usize, cap: usize| -> usize {
            used.saturating_add(delta).min(cap)
        };
        Self {
            bytes_read: combine(
                usage.read_size.total_document_size,
                budget.bytes_read,
                ceiling.bytes_read,
            ),
            documents_read: combine(
                usage.read_size.total_document_count,
                budget.documents_read,
                ceiling.documents_read,
            ),
            database_queries: combine(
                usage.num_intervals,
                budget.database_queries,
                ceiling.database_queries,
            ),
            documents_written: combine(
                usage.write_size.num_writes,
                budget.documents_written,
                ceiling.documents_written,
            ),
            bytes_written: combine(
                usage.write_size.size,
                budget.bytes_written,
                ceiling.bytes_written,
            ),
            functions_scheduled: combine(
                usage.scheduled_size.num_writes,
                budget.functions_scheduled,
                ceiling.functions_scheduled,
            ),
            scheduled_function_args_bytes: combine(
                usage.scheduled_size.size,
                budget.scheduled_function_args_bytes,
                ceiling.scheduled_function_args_bytes,
            ),
            files_written: combine(
                usage.file_storage_write_size.num_writes,
                budget.files_written,
                ceiling.files_written,
            ),
            file_write_bytes: combine(
                usage.file_storage_write_size.size,
                budget.file_write_bytes,
                ceiling.file_write_bytes,
            ),
            files_read: combine(
                usage.file_storage_read_size.num_reads,
                budget.files_read,
                ceiling.files_read,
            ),
            file_read_bytes: combine(
                usage.file_storage_read_size.size,
                budget.file_read_bytes,
                ceiling.file_read_bytes,
            ),
        }
    }
}
