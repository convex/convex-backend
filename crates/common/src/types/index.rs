use std::{
    borrow::Cow,
    fmt::{
        self,
        Debug,
        Display,
    },
    num::NonZeroU32,
    str::FromStr,
};

use anyhow::Context;
use compact_str::CompactString;
use sync_types::{
    identifier::{
        check_valid_identifier,
        MIN_IDENTIFIER,
    },
    Timestamp,
};
use value::{
    heap_size::HeapSize,
    identifier::is_valid_identifier,
    FieldName,
    InternalDocumentId,
    InternalId,
    ResolvedDocumentId,
    TableName,
    TabletId,
};

use crate::{
    bootstrap_model::index::{
        index_validation_error,
        DeveloperIndexMetadata,
    },
    document::ParsedDocument,
    index::IndexKey,
};

/// Descriptor for an index, e.g., "by_email".
#[derive(Clone, derive_more::Deref, derive_more::Display, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[display("{_0}")]
pub struct IndexDescriptor(CompactString);

impl IndexDescriptor {
    pub const MIN: Self = IndexDescriptor(CompactString::const_new(MIN_IDENTIFIER));

    pub fn is_reserved(&self) -> bool {
        *self == INDEX_BY_ID_DESCRIPTOR
            || *self == INDEX_BY_CREATION_TIME_DESCRIPTOR
            || self.0.starts_with('_')
    }

    pub fn new<S: Into<Cow<'static, str>>>(s: S) -> anyhow::Result<Self> {
        let cow: Cow<'static, str> = s.into();
        check_valid_identifier(&cow)
            .with_context(|| index_validation_error::invalid_index_name(&cow))?;
        Ok(Self(match cow {
            Cow::Borrowed(s) => CompactString::const_new(s),
            Cow::Owned(s) => s.into(),
        }))
    }

    /// Creates an IndexDescriptor from a string literal, panicking if invalid.
    /// This should only be used in a const context.
    ///
    /// Use [IndexDescriptor::new] for runtime input.
    pub const fn const_new(s: &'static str) -> Self {
        if !is_valid_identifier(s) {
            panic!("IndexDescriptor is not a valid identifier");
        }
        Self(CompactString::const_new(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IndexDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl From<IndexDescriptor> for String {
    fn from(t: IndexDescriptor) -> Self {
        t.0.to_string()
    }
}

impl HeapSize for IndexDescriptor {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl From<IndexDescriptor> for FieldName {
    fn from(desc: IndexDescriptor) -> Self {
        desc.0.parse().expect("IndexDescriptor not valid FieldName")
    }
}

/// Unique name for an index.
///
/// `Ord` orders by table, then index name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenericIndexName<T: IndexTableIdentifier> {
    table: T,
    descriptor: IndexDescriptor,
}

pub trait IndexTableIdentifier:
    Debug + Display + Clone + HeapSize + Ord + Eq + Sync + Send + 'static
{
}

impl IndexTableIdentifier for TableName {}
impl IndexTableIdentifier for TabletId {}

pub type IndexName = GenericIndexName<TableName>;

pub type TabletIndexName = GenericIndexName<TabletId>;

/// Like TabletIndexName in that it refers to a stable underlying index,
/// but it works for virtual tables too.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StableIndexName {
    Physical(TabletIndexName),
    Virtual(IndexName, TabletIndexName),
    Missing(IndexName),
}

impl StableIndexName {
    pub fn tablet_index_name(&self) -> Option<&TabletIndexName> {
        match self {
            StableIndexName::Physical(tablet_index_name) => Some(tablet_index_name),
            StableIndexName::Virtual(_, tablet_index_name) => Some(tablet_index_name),
            StableIndexName::Missing(_) => None,
        }
    }

    pub fn tablet_index_name_or_missing(&self) -> Result<&TabletIndexName, &IndexName> {
        match self {
            StableIndexName::Physical(tablet_index_name) => Ok(tablet_index_name),
            StableIndexName::Virtual(_, tablet_index_name) => Ok(tablet_index_name),
            StableIndexName::Missing(index_name) => Err(index_name),
        }
    }
}

impl HeapSize for TabletIndexName {
    fn heap_size(&self) -> usize {
        self.descriptor.heap_size()
    }
}

impl<T: IndexTableIdentifier + FromStr<Err = anyhow::Error>> FromStr for GenericIndexName<T> {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.');
        let table: T = match parts.next() {
            Some(s) => s
                .parse()
                .with_context(|| index_validation_error::invalid_table_name(s))?,
            None => anyhow::bail!(index_validation_error::not_enough_name_components(s)),
        };
        let descriptor = match parts.next() {
            Some(s) => IndexDescriptor::new(s.to_string())
                .with_context(|| index_validation_error::invalid_table_name(s))?,
            None => anyhow::bail!(index_validation_error::not_enough_name_components(s)),
        };
        if parts.next().is_some() {
            anyhow::bail!(index_validation_error::too_many_name_components(s));
        }
        Ok(Self { table, descriptor })
    }
}

/// Diff of the indexes between two versions of schema.ts
/// Index can be
/// - missing (not present in schema.ts)
/// - staged (exists with { staged: true })
/// - exist (exists with { staged: false } or without the staged field)
///
/// Note that this is unrelated to the storage state
#[derive(Debug, Clone, Default)]
pub struct IndexDiff {
    /// missing -> exists
    /// missing -> staged
    pub added: Vec<DeveloperIndexMetadata>,
    /// staged -> staged
    /// exist -> exist
    pub identical: Vec<ParsedDocument<DeveloperIndexMetadata>>,
    /// staged -> missing
    /// exist -> missing
    pub dropped: Vec<ParsedDocument<DeveloperIndexMetadata>>,
    /// staged -> exist
    pub enabled: Vec<ParsedDocument<DeveloperIndexMetadata>>,
    /// exist -> staged
    pub disabled: Vec<ParsedDocument<DeveloperIndexMetadata>>,
}

impl IndexDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.dropped.is_empty()
            && self.enabled.is_empty()
            && self.disabled.is_empty()
    }
}

impl<T: IndexTableIdentifier> fmt::Display for GenericIndexName<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.table, self.descriptor)
    }
}

impl<T: IndexTableIdentifier> fmt::Debug for GenericIndexName<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.table, self.descriptor)
    }
}

pub const INDEX_BY_ID_DESCRIPTOR: IndexDescriptor = IndexDescriptor::const_new("by_id");
pub const INDEX_BY_CREATION_TIME_DESCRIPTOR: IndexDescriptor =
    IndexDescriptor::const_new("by_creation_time");

impl<T: IndexTableIdentifier> GenericIndexName<T> {
    /// Create a new index name for the table and given descriptor,
    /// e.g., "users.by_email".
    pub fn new(table: T, descriptor: IndexDescriptor) -> anyhow::Result<Self> {
        check_valid_identifier(&descriptor)?;
        anyhow::ensure!(
            !descriptor.is_reserved(),
            index_validation_error::name_reserved(&table, &descriptor)
        );
        Ok(Self { table, descriptor })
    }

    pub fn new_reserved(table: T, descriptor: IndexDescriptor) -> anyhow::Result<Self> {
        check_valid_identifier(&descriptor)?;
        anyhow::ensure!(
            descriptor.is_reserved(),
            "Index descriptor is not reserved: {descriptor}"
        );
        Ok(Self { table, descriptor })
    }

    /// The index that exists for all tables which indexes no fields except the
    /// implicitly-included `_id`.
    pub const fn by_id(table: T) -> Self {
        Self {
            table,
            descriptor: INDEX_BY_ID_DESCRIPTOR,
        }
    }

    /// The index that exists for all tables which indexes `_creationTime`.
    pub const fn by_creation_time(table: T) -> Self {
        Self {
            table,
            descriptor: INDEX_BY_CREATION_TIME_DESCRIPTOR,
        }
    }

    /// The least possible index name (by `Ord` ordering) for the given table.
    pub fn min_for_table(table: T) -> Self {
        Self {
            table,
            descriptor: IndexDescriptor::MIN,
        }
    }

    /// The table this index is over.
    pub fn table(&self) -> &T {
        &self.table
    }

    /// The descriptor for the index, e.g., "by_email".
    pub fn descriptor(&self) -> &IndexDescriptor {
        &self.descriptor
    }

    /// Is the index name for the by_id index?
    pub fn is_by_id(&self) -> bool {
        self.descriptor == INDEX_BY_ID_DESCRIPTOR
    }

    /// Is the index name for the creation time index?
    pub fn is_creation_time(&self) -> bool {
        self.descriptor == INDEX_BY_CREATION_TIME_DESCRIPTOR
    }

    /// Is this index reserved? The system automatically defines these indexes
    /// on all tables and therefore allows querying them if the underlying table
    /// doesn't exist.
    ///
    /// Note - this is subtly different than IndexName.is_system_owned because
    /// this method only checks the index name, but IndexName.is_system_owned
    /// checks both the table name and the index name. This method is not
    /// sufficient to determine if an index can be mutated safely, you must
    /// resolve the table id and check the table id.
    pub fn is_by_id_or_creation_time(&self) -> bool {
        self.is_by_id() || self.is_creation_time()
    }

    pub fn map_table<U: IndexTableIdentifier, E>(
        self,
        f: &impl Fn(T) -> Result<U, E>,
    ) -> Result<GenericIndexName<U>, E> {
        Ok(GenericIndexName {
            table: f(self.table)?,
            descriptor: self.descriptor,
        })
    }
}

impl IndexName {
    /// Is this index either an index on a system table or a system-defined
    /// index? These indexes do not count towards user quota.
    pub fn is_system_owned(&self) -> bool {
        // Table scan and all indexes on system tables do not count towards
        // user defined indexes.
        self.table.is_system() || self.descriptor.is_reserved()
    }

    pub fn to_resolved(
        self,
        f: impl Fn(TableName) -> anyhow::Result<TabletId>,
    ) -> anyhow::Result<TabletIndexName> {
        Ok(GenericIndexName {
            table: f(self.table)?,
            descriptor: self.descriptor,
        })
    }
}

/// The ID of an index is the internal ID of its index metadata document's ID
/// (the document in the `_index` table).
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    derive_more::From,
    derive_more::Display,
)]
pub struct IndexId(pub InternalId);
impl IndexId {
    pub fn size(&self) -> usize {
        self.0.size()
    }
}

/// The compact identifier used by a persistence implementation to store an
/// index's rows.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PersistenceIndexId(NonZeroU32);

impl PersistenceIndexId {
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn value(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for PersistenceIndexId {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> anyhow::Result<Self> {
        Self::new(value).context("persistence index IDs must be nonzero")
    }
}

/// A database index as persistence identifies it: the `_index` document's ID
/// and the persistence index ID its metadata held at the snapshot being read,
/// so a layout keyed by that ID never resolves it from newer metadata. Text and
/// vector indexes have no persistence rows and so no ref.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IndexRef {
    id: IndexId,
    /// `None` until the index's metadata is assigned one, or without metadata.
    /// Non-optional once every index has one and every ref comes from metadata.
    persistence_index_id: Option<PersistenceIndexId>,
}

impl IndexRef {
    /// A ref built without metadata, which a layout keyed by the persistence
    /// index ID rejects. Its callers are the fallback for a funrun predating
    /// the ID on the wire and `db_import`, whose exports carry none; when both
    /// are gone, so is this, and the ID stops being optional.
    pub const fn unresolved(id: IndexId) -> Self {
        Self {
            id,
            persistence_index_id: None,
        }
    }

    /// Reassembles a ref whose parts came from one ref or one metadata
    /// revision.
    pub const fn from_parts(id: IndexId, persistence_index_id: Option<PersistenceIndexId>) -> Self {
        Self {
            id,
            persistence_index_id,
        }
    }

    pub const fn id(&self) -> IndexId {
        self.id
    }

    pub const fn persistence_index_id(&self) -> Option<PersistenceIndexId> {
        self.persistence_index_id
    }
}

// TODO: this encoding is rarely used and confusing
impl From<IndexId> for String {
    fn from(index_id: IndexId) -> String {
        index_id.0.into()
    }
}
impl FromStr for IndexId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum IndexWriteMode {
    /// Index backfill has inserted all existing index entries. In persistence
    /// v6, `DatabaseIndexUpdate::prev` (copied to
    /// `PersistenceIndexEntry::prev`) identifies the row to remove from the
    /// current index and store in history, using its `ts` and `document_id`
    /// for the update's `index` and `key`.
    ScanComplete,
    /// Index backfill is still inserting entries, so `prev` may not exist in
    /// persistence yet. Persistence v6 records deletions for post-scan
    /// reconciliation instead of verifying `prev`.
    Scanning,
}

#[derive(Eq, PartialEq, Clone, Debug, Ord, PartialOrd)]
pub struct DatabaseIndexUpdate {
    pub mode: IndexWriteMode,
    pub index: IndexRef,

    pub key: IndexKey,
    pub value: DatabaseIndexValue,
    /// The entry this update supersedes: the old document's entry for the same
    /// key, when it produced one.
    pub prev: Option<PrevIndexEntry>,

    pub is_system_index: bool,
}

/// An index entry a newer one supersedes: the same key's previous revision. A
/// layout that keeps one live row per key moves that row into its history from
/// these values instead of reading it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrevIndexEntry {
    pub ts: Timestamp,
    pub document_id: InternalDocumentId,
}

#[derive(Eq, PartialEq, Clone, Debug, Ord, PartialOrd)]
pub enum DatabaseIndexValue {
    // We don't need the documentId in case of index entry delete.
    Deleted,
    // Non-clustered index only stores the documentId.
    NonClustered(ResolvedDocumentId),
}

impl DatabaseIndexValue {
    pub fn is_delete(&self) -> bool {
        matches!(self, DatabaseIndexValue::Deleted)
    }
}
