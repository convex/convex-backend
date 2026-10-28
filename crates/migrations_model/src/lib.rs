//! This crate exists to duplicate logic from the model crate
//! so that we can evolve model over time, while freezing the old version of the
//! model code in time at the time of the migration. This allows migrations to
//! continue to work long after the model has been updated.

#![feature(coroutines)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
#![feature(impl_trait_in_assoc_type)]

use std::{
    borrow::Cow,
    fmt,
};

use common::runtime::Runtime;
use database::{
    Database,
    TableModel,
};
use value::{
    TableName,
    TableNamespace,
};

pub mod migr_119;
pub mod migr_121;
pub mod migr_124;
pub mod migr_125;
pub mod migr_130;
pub mod migr_133;

pub type DatabaseVersion = i64;
// The version for the format of the database. We support all previous
// migrations unless explicitly dropping support.
// Add a user name next to the version when you make a change to highlight merge
// conflicts.
pub const DATABASE_VERSION: DatabaseVersion = 134; // nipunn

pub struct MigrationExecutor<RT: Runtime> {
    pub db: Database<RT>,
}

impl<RT: Runtime> MigrationExecutor<RT> {
    pub async fn perform_migration(&self, to_version: DatabaseVersion) -> anyhow::Result<()> {
        let completion_criterion = match to_version {
            1..=116 => panic!("Transition too old!"),
            117 => {
                let backend_serving_record_table: TableName = "_backend_serving_record"
                    .parse()
                    .expect("Invalid built-in backend_serving_record table");
                let mut tx = self.db.begin_system().await?;
                TableModel::new(&mut tx)
                    .delete_active_table(TableNamespace::Global, backend_serving_record_table)
                    .await?;
                self.db
                    .commit_with_write_source(tx, "migration_117")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            // Empty migration for 118 - represents creation of CronNextRun table
            118 => MigrationCompletionCriterion::MigrationComplete(to_version),
            119 => {
                let mut tx = self.db.begin_system().await?;
                migr_119::run_migration(&mut tx).await?;
                self.db
                    .commit_with_write_source(tx, "migration_119")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            120 => {
                // This is an empty migration because we added a new system
                // table, _index_backfills
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            121 => {
                let mut tx = self.db.begin_system().await?;
                migr_121::run_migration(&mut tx).await?;
                self.db
                    .commit_with_write_source(tx, "migration_121")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            122 => {
                // This is an empty migration because we added a new system
                // table for each component, _schema_validation_progress
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            123 => {
                // This is an empty migration because we added a new system
                // table for each component, _scheduled_job_args
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            124 => {
                let mut tx = self.db.begin_system().await?;
                migr_124::run_migration(&mut tx).await?;
                self.db
                    .commit_with_write_source(tx, "migration_124")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            125 => {
                let mut tx = self.db.begin_system().await?;
                migr_125::run_migration(&mut tx).await?;
                self.db
                    .commit_with_write_source(tx, "migration_125")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            126 => {
                // This is an empty migration because we added a new system
                // table, _usage_limits.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            127 => {
                // This is an empty migration because we added a new system
                // table, _data_sync_progress.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            128 => {
                // This is an empty migration because we added the
                // by_action_and_creation_time index to _deployment_audit_log,
                // which is backfilled automatically on startup.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            129 => {
                // The application system-table initializer creates the persistence index ID
                // allocator before this migration runs.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            130 => {
                // Backfill in bounded batches, resuming the `_index` scan from
                // the previous batch's cursor so the table is scanned once
                // rather than re-scanned from the start each batch. Each batch
                // stays under the per-transaction read and write caps; rerun
                // until the scan reaches the end.
                let mut cursor = None;
                loop {
                    let mut tx = self.db.begin_system().await?;
                    let progress = migr_130::run_migration(&mut tx, cursor).await?;
                    if progress.patched > 0 {
                        self.db
                            .commit_with_write_source(tx, "migration_130")
                            .await?;
                    }
                    let Some(next) = progress.cursor else {
                        break;
                    };
                    cursor = Some(next);
                }
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            // NOTE: Make sure to increase DATABASE_VERSION when adding new migrations.
            131 => {
                // Bootstrap creates _schema_validations and its indexes before migration runs.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            132 => {
                // Bootstrap creates by_validation_id before attempt-owned counters are written.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            133 => {
                let mut tx = self.db.begin_system().await?;
                migr_133::run_migration(&mut tx).await?;
                self.db
                    .commit_with_write_source(tx, "migration_133")
                    .await?;
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            134 => {
                // Bootstrap creates _s3_export_state and its index before migration runs.
                MigrationCompletionCriterion::MigrationComplete(to_version)
            },
            _ => anyhow::bail!("Version did not define a migration! {}", to_version),
        };
        tracing::warn!(
            "Executing Migration {}/{}. {}",
            to_version,
            DATABASE_VERSION,
            completion_criterion
        );
        Ok(())
    }
}

pub enum MigrationCompletionCriterion {
    /// Committing the migration in migrations.rs is sufficient.
    MigrationComplete(DatabaseVersion),
    /// Some other log line printed out, e.g. by a background worker creating
    /// a new index or a backfill.
    LogLine(Cow<'static, str>),
}

impl fmt::Display for MigrationCompletionCriterion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationCompletionCriterion::MigrationComplete(version) => {
                write!(f, "Wait for log line 'Migrated {version}'")
            },
            MigrationCompletionCriterion::LogLine(line) => write!(f, "Wait for log line '{line}'"),
        }
    }
}
