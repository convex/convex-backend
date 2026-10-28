//! Write set tracking for an active transaction
use std::{
    borrow::Borrow,
    cmp::Ordering,
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::{
        database_index::IndexedFields,
        index_metadata_serialize_tablet_id,
        INDEX_BY_TABLE_ID_VIRTUAL_INDEX_DESCRIPTOR,
        TABLE_ID_FIELD_PATH,
    },
    document::{
        PendingDocument,
        PendingDocumentUpdate,
        ResolvedDocument,
    },
    index::IndexKey,
    interval::{
        BinaryKey,
        Interval,
    },
    knobs::{
        TRANSACTION_MAX_SYSTEM_NUM_WRITES,
        TRANSACTION_MAX_SYSTEM_WRITE_SIZE_BYTES,
    },
    types::{
        TabletIndexName,
        WriteTimestamp,
    },
    value::ResolvedDocumentId,
};
use errors::ErrorMetadata;
use imbl::OrdSet;
use value::{
    values_to_bytes,
    DeveloperDocumentId,
    TabletId,
};

use crate::{
    bootstrap_model::defaults::BootstrapTableIds,
    execution_size::TransactionLimits,
    reads::TransactionReadSet,
    schema_registry::SchemaRegistry,
    ComponentRegistry,
    TableRegistry,
};

pub trait PendingWrites: Clone {}

impl PendingWrites for Writes {}
impl PendingWrites for TableRegistry {}
impl PendingWrites for SchemaRegistry {}
impl PendingWrites for ComponentRegistry {}

pub type NestedWriteToken = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct NestedWrites<W: PendingWrites> {
    parent: Option<Box<NestedWrites<W>>>,
    pending: W,
    nested_token: NestedWriteToken,
}

impl<W: PendingWrites> Deref for NestedWrites<W> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.pending
    }
}

impl<W: PendingWrites> DerefMut for NestedWrites<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pending
    }
}

impl<W: PendingWrites> NestedWrites<W> {
    pub fn new(writes: W) -> Self {
        Self {
            parent: None,
            pending: writes,
            nested_token: 0,
        }
    }

    pub fn begin_nested(&mut self) -> NestedWriteToken {
        let new_pending = self.pending.clone();
        let nested_token = self.nested_token + 1;
        let new_writes = NestedWrites {
            parent: None,
            pending: new_pending,
            nested_token,
        };
        let parent = std::mem::replace(self, new_writes);
        self.parent = Some(Box::new(parent));
        nested_token
    }

    pub fn commit_nested(&mut self, token: NestedWriteToken) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.nested_token == token,
            "Mismatched nested transaction token {} != {}",
            self.nested_token,
            token
        );
        let parent = self
            .parent
            .take()
            .context("No nested transaction to commit")?;
        let pending = std::mem::replace(self, *parent).pending;
        self.pending = pending;
        Ok(())
    }

    pub fn rollback_nested(&mut self, token: NestedWriteToken) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.nested_token == token,
            "Mismatched nested transaction token {} != {}",
            self.nested_token,
            token
        );
        let parent = self
            .parent
            .take()
            .context("No nested transaction to rollback")?;
        *self = *parent;
        Ok(())
    }

    pub fn require_not_nested(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.parent.is_none(), "Nested transaction in progress");
        Ok(())
    }

    pub fn as_flat(&self) -> anyhow::Result<&W> {
        self.require_not_nested()?;
        Ok(&self.pending)
    }

    pub fn into_flat(self) -> anyhow::Result<W> {
        self.require_not_nested()?;
        Ok(self.pending)
    }

    pub fn pending(&mut self) -> &mut W {
        &mut self.pending
    }
}

#[derive(Debug, Clone)]
struct Update(Arc<PendingDocumentUpdate>);

impl Deref for Update {
    type Target = PendingDocumentUpdate;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Ord for Update {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.id().cmp(&other.0.id())
    }
}

impl PartialOrd for Update {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Update {}

impl PartialEq for Update {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Borrow<ResolvedDocumentId> for Update {
    fn borrow(&self) -> &ResolvedDocumentId {
        self.0.id_ref()
    }
}

/// The write set for a transaction, maintained by `TransactionState`
#[derive(Debug, Clone, PartialEq)]
pub struct Writes {
    // N.B.: OrdMap/OrdSet are very sensitive to the size of keys and values (as
    // a map stores a minimum of 64 key-value pairs, even if empty) and likes to
    // clone them at will, so we store just a single Arc inside of it
    updates: OrdSet<Update>,

    // Fields below can be recomputed from `updates`.

    // Size of writes to user tables
    user_tx_size: TransactionWriteSize,
    // Size of writes to system tables
    system_tx_size: TransactionWriteSize,
    // New `_index` documents, tracked so commit finalization can assign a
    // persistence ID to database indexes that do not already have one.
    new_index_document_ids: OrdSet<ResolvedDocumentId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionWriteSize {
    // Total number of writes (i.e. calls to `mutate`)
    pub num_writes: usize,

    // Total size of mutations. Writing to the same DocumentId twice will still count twice.
    pub size: usize,
}

impl Writes {
    /// Create an empty write set.
    pub fn new() -> Self {
        Self {
            updates: OrdSet::new(),
            user_tx_size: TransactionWriteSize::default(),
            system_tx_size: TransactionWriteSize::default(),
            new_index_document_ids: OrdSet::new(),
        }
    }

    /// Are there any writes in the active transaction?
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    pub fn update(
        &mut self,
        bootstrap_tables: BootstrapTableIds,
        is_system_document: bool,
        reads: &mut TransactionReadSet,
        document_id: ResolvedDocumentId,
        old_document: Option<(ResolvedDocument, WriteTimestamp)>,
        new_document: Option<PendingDocument>,
        limits: &TransactionLimits,
    ) -> anyhow::Result<()> {
        if old_document.is_none() {
            anyhow::ensure!(!self.updates.contains(&document_id), "Duplicate insert");
            self.register_new_id(reads, document_id)?;
        }
        Self::record_reads_for_write(bootstrap_tables, reads, document_id.tablet_id)?;

        let value_size = match &new_document {
            Some(document) => document.size_and_nesting().0,
            None => 0,
        };

        let tx_size = if is_system_document {
            &mut self.system_tx_size
        } else {
            &mut self.user_tx_size
        };

        // We always increment the size first, even if we throw,
        // we want the size to reflect the write, so that
        // we can tell that we threw and not issue a warning.
        tx_size.num_writes += 1;
        tx_size.size += value_size;

        if is_system_document {
            let tx_size = &self.system_tx_size;
            // If we exceed system limits, throw a system error and not a developer one.
            // Developers have no control over system tables. We should define feature
            // specific limit to avoid hitting the system table ones if needed.
            anyhow::ensure!(
                tx_size.num_writes <= *TRANSACTION_MAX_SYSTEM_NUM_WRITES,
                "Too many system document writes in a single transaction: {}",
                tx_size.num_writes
            );
            anyhow::ensure!(
                tx_size.size <= *TRANSACTION_MAX_SYSTEM_WRITE_SIZE_BYTES,
                "Too many bytes written in system tables in a single transaction: {}",
                tx_size.size
            );
        } else {
            let tx_size = &self.user_tx_size;
            anyhow::ensure!(
                tx_size.num_writes <= limits.documents_written,
                ErrorMetadata::pagination_limit(
                    "TooManyWrites",
                    format!(
                        "Too many writes in a single function execution (limit: {})",
                        limits.documents_written,
                    )
                ),
            );
            anyhow::ensure!(
                tx_size.size <= limits.bytes_written,
                ErrorMetadata::pagination_limit(
                    "TooManyBytesWritten",
                    format!(
                        "Too many bytes written in a single function execution (limit: {})",
                        common::fmt::format_bytes(limits.bytes_written as u64),
                    )
                ),
            );
        }

        // The `prev` document/timestamp stored on the new update: the revision
        // this write replaces in the document log.
        let prev = match self.updates.get(&document_id) {
            // Coalescing a later write onto one already staged in this
            // transaction: keep the originally staged `prev`.
            Some(old_update) => {
                let (old_document, old_document_ts) = old_document.unzip();
                anyhow::ensure!(
                    old_update.new_document_with_max_commit_ts()?.as_deref()
                        == old_document.as_ref(),
                    "Inconsistent update: The old update's new document does not match the new \
                     document's old update"
                );
                anyhow::ensure!(
                    [None, Some(WriteTimestamp::Pending)].contains(&old_document_ts),
                    "Inconsistent update: The new document's old update timestamp should be \
                     Pending but is {:?}",
                    old_document_ts
                );
                old_update.old_document().cloned()
            },
            // First write to this document in this transaction: its `prev` is
            // the committed revision it read.
            None => match old_document {
                None => None,
                Some((d, WriteTimestamp::Committed(ts))) => Some((d, ts)),
                Some((_, WriteTimestamp::Pending)) => anyhow::bail!(
                    "Old document timestamp is Pending, but there is no pending write"
                ),
            },
        };
        if document_id.tablet_id == bootstrap_tables.index_id.tablet_id {
            if prev.is_none() && new_document.is_some() {
                self.new_index_document_ids.insert(document_id);
            } else {
                // if the index was created and deleted in the same transaction, we don't
                // want to bother creating a new index id for it.
                self.new_index_document_ids.remove(&document_id);
            }
        }
        self.updates
            .insert(Update(Arc::new(PendingDocumentUpdate::new(
                document_id,
                prev,
                new_document,
            ))));

        Ok(())
    }

    fn record_reads_for_write(
        table_mapping: BootstrapTableIds,
        reads: &mut TransactionReadSet,
        tablet_id: TabletId,
    ) -> anyhow::Result<()> {
        // by_name index on _indexes table.
        if table_mapping.is_index_table(tablet_id) || table_mapping.is_tables_table(tablet_id) {
            // Changes in _tables or _index cannot race with any other table or
            // index. This is because TableRegistry and IndexRegistry check a
            // number of invariants between tables and index records.
            // TODO(presley): This is probably the wrong layer to add this dependency.
            // We should be added by TableRegistry and IndexRegistry themselves.
            // For example, fast forwarding a vector search checkpoint does not
            // need this dependency.
            reads.record_indexed_derived(
                TabletIndexName::by_id(table_mapping.tables_id.tablet_id),
                IndexedFields::by_id(),
                Interval::all(),
            );
            reads.record_indexed_derived(
                TabletIndexName::by_id(table_mapping.index_id.tablet_id),
                IndexedFields::by_id(),
                Interval::all(),
            );
        } else {
            // Writes to a table require the table still exists.
            let table_id_bytes = IndexKey::new(
                vec![],
                DeveloperDocumentId::new(table_mapping.tables_id.table_number, tablet_id.0),
            )
            .to_bytes();
            reads.record_indexed_derived(
                TabletIndexName::by_id(table_mapping.tables_id.tablet_id),
                IndexedFields::by_id(),
                Interval::prefix(table_id_bytes.into()),
            );

            // Inserts or updates also need all of the indexes they touch to
            // be stable. Thus we take read dependency on all indexes for that table_id.
            // TODO(presley): The _index.by_table_id index does not really exist.
            // Pretend it does since evaluating read dependencies do not actually
            // need to read the index. We only care about the name always mapping
            // to the same fields.
            let tablet_id_bytes =
                values_to_bytes(&[Some(index_metadata_serialize_tablet_id(&tablet_id)?)]);
            reads.record_indexed_derived(
                TabletIndexName::new(
                    table_mapping.index_id.tablet_id,
                    INDEX_BY_TABLE_ID_VIRTUAL_INDEX_DESCRIPTOR.clone(),
                )?,
                vec![TABLE_ID_FIELD_PATH.clone()].try_into()?,
                // Note that should really be exact point instead of a prefix,
                // but our read set interval does not support this.
                Interval::prefix(BinaryKey::from(tablet_id_bytes)),
            );
        };

        Ok(())
    }

    /// Register a newly allocated DocumentId.
    /// This enables us to check for reuse on commit.
    pub(crate) fn register_new_id(
        &mut self,
        reads: &mut TransactionReadSet,
        document_id: ResolvedDocumentId,
    ) -> anyhow::Result<()> {
        // New ID creation requires the ID to have never existed before.
        // We check in CommitterClient that it never existed before the transaction's
        // begin timestamp, and here we take a dependency on the ID to make sure
        // it cannot be created by a parallel commit.
        let index_name = TabletIndexName::by_id(document_id.tablet_id);
        let id_bytes = IndexKey::new(vec![], document_id.into()).to_bytes();
        reads.record_indexed_derived(
            index_name,
            IndexedFields::by_id(),
            Interval::prefix(id_bytes.into()),
        );
        Ok(())
    }

    /// How large is the given write transaction?
    pub fn user_size(&self) -> &TransactionWriteSize {
        &self.user_tx_size
    }

    pub fn system_size(&self) -> &TransactionWriteSize {
        &self.system_tx_size
    }

    /// Iterate over the coalesced writes (so no `DocumentId` appears twice).
    pub fn coalesced_writes(&self) -> impl Iterator<Item = &PendingDocumentUpdate> {
        self.updates.iter().map(|x| &**x)
    }

    pub fn into_coalesced_writes(self) -> impl Iterator<Item = Arc<PendingDocumentUpdate>> {
        self.updates.into_iter().map(|x| x.0)
    }

    pub fn into_updates(
        self,
    ) -> OrdSet<impl Borrow<ResolvedDocumentId> + Ord + Deref<Target = PendingDocumentUpdate>> {
        self.updates
    }

    /// The staged write for `id`, if any.
    pub fn get(&self, id: &ResolvedDocumentId) -> Option<&PendingDocumentUpdate> {
        self.updates.get(id).map(|update| &**update)
    }

    pub(crate) fn new_index_document_ids(&self) -> impl Iterator<Item = ResolvedDocumentId> + '_ {
        self.new_index_document_ids.iter().copied()
    }

    pub fn generated_ids(&self) -> Vec<ResolvedDocumentId> {
        self.updates
            .iter()
            .filter(|update| update.old_document().is_none())
            .map(|update| update.id())
            .collect()
    }
}
