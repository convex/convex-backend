use std::{
    cmp::{
        Ord,
        Ordering,
        PartialOrd,
    },
    fmt::{
        self,
        Debug,
        Display,
    },
    ops::Deref,
    str::FromStr,
};

use anyhow::Context;
use errors::ErrorMetadata;
use serde_json::Value as JsonValue;

use crate::{
    base64::{
        decode_urlsafe,
        encode_urlsafe,
    },
    heap_size::HeapSize,
    sha256::Sha256,
    Size,
    TableNumber,
    TabletId,
};

/// A raw reference to a document. `DocumentId`s can appear in `Value`s as
/// `Id`s, `StrongRef`s, or `WeakRef`s.
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Copy)]
pub struct InternalDocumentId {
    table: TabletId,
    internal_id: InternalId,
}

#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Copy)]
pub struct DeveloperDocumentId {
    table: TableNumber,
    internal_id: InternalId,
}

#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Copy, Debug)]
pub struct ResolvedDocumentId {
    pub tablet_id: TabletId,
    pub developer_id: DeveloperDocumentId,
}

impl InternalDocumentId {
    pub const MAX: InternalDocumentId = InternalDocumentId::new(TabletId::MAX, InternalId::MAX);
    /// Minimum valid [`InternalDocumentId`].
    pub const MIN: InternalDocumentId = InternalDocumentId::new(TabletId::MIN, InternalId::MIN);

    /// Create a new [`InternalDocumentId`] from a [`TabletId`] and an
    /// [`InternalId`].
    pub const fn new(table: TabletId, internal_id: InternalId) -> Self {
        Self { table, internal_id }
    }

    /// The table that the reference points into.
    pub fn table(&self) -> TabletId {
        self.table
    }

    /// The ID of the document the reference points at.
    pub fn internal_id(&self) -> InternalId {
        self.internal_id
    }

    /// How large is the given `DocumentId`?
    pub fn size(&self) -> usize {
        self.table.size() + self.internal_id.size()
    }

    pub fn into_table_and_id(self) -> (TabletId, InternalId) {
        (self.table, self.internal_id)
    }
}

impl DeveloperDocumentId {
    pub const MAX: DeveloperDocumentId =
        DeveloperDocumentId::new(TableNumber::MAX, InternalId::MAX);
    /// Minimum valid [`DeveloperDocumentId`].
    pub const MIN: DeveloperDocumentId =
        DeveloperDocumentId::new(TableNumber::MIN, InternalId::MIN);

    /// Create a new [`DeveloperDocumentId`] from a [`TableNumber`] and an
    /// [`InternalId`].
    pub const fn new(table: TableNumber, internal_id: InternalId) -> Self {
        Self { table, internal_id }
    }

    /// The table that the reference points into.
    pub fn table(&self) -> TableNumber {
        self.table
    }

    /// The ID of the document the reference points at.
    pub fn internal_id(&self) -> InternalId {
        self.internal_id
    }

    /// How large is the given `DocumentId`?
    pub fn size(&self) -> usize {
        self.table.size() + self.internal_id.size()
    }

    pub fn into_table_and_id(self) -> (TableNumber, InternalId) {
        (self.table, self.internal_id)
    }
}

impl Debug for InternalDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl Debug for DeveloperDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl From<InternalDocumentId> for String {
    fn from(id: InternalDocumentId) -> Self {
        id.table().document_id_to_string(id.internal_id)
    }
}

impl From<DeveloperDocumentId> for String {
    fn from(id: DeveloperDocumentId) -> Self {
        id.table().document_id_to_string(id.internal_id)
    }
}

impl From<InternalDocumentId> for JsonValue {
    fn from(id: InternalDocumentId) -> JsonValue {
        serde_json::Value::String(id.into())
    }
}

impl From<DeveloperDocumentId> for JsonValue {
    fn from(id: DeveloperDocumentId) -> JsonValue {
        serde_json::Value::String(id.into())
    }
}

impl Display for InternalDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from(*self))
    }
}

impl Display for DeveloperDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from(*self))
    }
}

impl FromStr for InternalDocumentId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id: Result<Self, anyhow::Error> = try bikeshed anyhow::Result<_> {
            // Versions 4/5: DocumentIds have the table name and internal ID separated by
            // "|". Version 2 are 3 IDs are base64 encoded with an alphabet that
            // doesn't include "|".
            let (table_s, id_s) = s.split_once('|').context("Document IDs must contain '|'")?;
            Self {
                table: table_s.parse()?,
                internal_id: InternalId::from_str(id_s)?,
            }
        };
        id.context(ErrorMetadata::bad_request(
            "InvalidId",
            format!("Id `{s}` was invalid."),
        ))
    }
}

impl HeapSize for InternalDocumentId {
    fn heap_size(&self) -> usize {
        self.table.heap_size()
    }
}

impl HeapSize for DeveloperDocumentId {
    fn heap_size(&self) -> usize {
        self.table.heap_size()
    }
}

impl From<ResolvedDocumentId> for InternalDocumentId {
    fn from(value: ResolvedDocumentId) -> Self {
        InternalDocumentId::new(value.tablet_id, value.developer_id.internal_id)
    }
}

impl ResolvedDocumentId {
    pub const MIN: ResolvedDocumentId =
        ResolvedDocumentId::new(TabletId::MIN, DeveloperDocumentId::MIN);

    pub const fn new(tablet_id: TabletId, developer_id: DeveloperDocumentId) -> Self {
        Self {
            tablet_id,
            developer_id,
        }
    }

    pub fn internal_id(&self) -> InternalId {
        self.developer_id.internal_id
    }

    pub fn size(&self) -> usize {
        self.tablet_id.size() + self.developer_id.size()
    }
}

impl HeapSize for ResolvedDocumentId {
    fn heap_size(&self) -> usize {
        0
    }
}

impl Display for ResolvedDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.developer_id)
    }
}

impl InternalDocumentId {
    pub fn to_resolved(&self, table_number: TableNumber) -> ResolvedDocumentId {
        ResolvedDocumentId {
            tablet_id: self.table,
            developer_id: DeveloperDocumentId::new(table_number, self.internal_id),
        }
    }
}

/// An internal ID serialized to 16 bytes.
///
/// 14 bytes of randomness followed by the day encoded in 2 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternalId(pub [u8; 16]);

impl InternalId {
    pub fn size(&self) -> usize {
        16
    }
}

impl PartialOrd for InternalId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InternalId {
    fn cmp(&self, other: &Self) -> Ordering {
        self[..].cmp(&other[..])
    }
}

impl InternalId {
    /// A byte array that sorts after the bytes of every InternalId.
    pub const AFTER_ALL_BYTES: &'static [u8] = &[u8::MAX; 18];
    /// A byte array that sorts before the bytes of every InternalId.
    pub const BEFORE_ALL_BYTES: &'static [u8] = &[];
    /// The maximum valid InternalId in sorted order.
    pub const MAX: InternalId = Self([u8::MAX; 16]);
    pub const MAX_SIZE: usize = 17;
    /// The minimum valid InternalId in sorted order.
    pub const MIN: InternalId = Self([u8::MIN; 16]);

    pub fn from_developer_str(s: &str) -> anyhow::Result<Self> {
        // Developers apps may still provide V4 IDs, which are 17 bytes base 62 encoded,
        // or V5 IDs, which are 16 bytes base 64 encoded.

        // All IDs have been migrated to use the latter, so we parse both but only
        // return the V5 version.

        // The V4 encoder uses the `base_62` crate to encode 17 bytes with the change of
        // base algorithm. Note that log(256)/log(62) * 17 is 22.84, so the
        // output must be at least 23 bytes.
        let result = if s.len() > 22 {
            let vec = base_62::decode(s).map_err(|e| anyhow::anyhow!("Failed decode ID {e}"))?;
            let v4_bytes: [u8; 17] = vec[..].try_into()?;
            let randomness: [u8; 15] = v4_bytes[..15]
                .try_into()
                .expect("Could not read bytes from InternalId");
            let day: [u8; 2] = v4_bytes[15..17]
                .try_into()
                .expect("Could not read bytes from InternalId");
            let truncated: [u8; 14] = Sha256::hash(&randomness)[..14]
                .try_into()
                .expect("Could not truncate V4 ID");
            let mut reconstructed = [0u8; 16];
            reconstructed[..14].copy_from_slice(&truncated);
            reconstructed[14..16].copy_from_slice(&day);
            InternalId(reconstructed)
        }
        // Similarly, log(256)/log(64) * 16 is 21.33, so `base64` will output 22 bytes.
        else {
            let vec = decode_urlsafe(s)?;
            InternalId(vec[..].try_into()?)
        };
        Ok(result)
    }
}

impl Display for InternalId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", String::from(*self))
    }
}

impl Debug for InternalId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InternalId({self})")
    }
}

impl Deref for InternalId {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for InternalId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // All IDs in our system should be V5 IDs, which are 16 bytes base 64 encoded.
        // If parsing a developer provided ID, use `from_developer_str` which handles
        // the older V4 format.
        if s.len() > 22 {
            anyhow::bail!("Found V4 Internal ID");
        }
        let vec = decode_urlsafe(s)?;
        Ok(InternalId(vec[..].try_into()?))
    }
}

impl From<InternalId> for String {
    fn from(id: InternalId) -> Self {
        encode_urlsafe(&id.0[..])
    }
}

impl From<[u8; 16]> for InternalId {
    fn from(id: [u8; 16]) -> Self {
        InternalId(id)
    }
}

impl From<InternalId> for Vec<u8> {
    fn from(id: InternalId) -> Vec<u8> {
        id.0.into()
    }
}

impl<'a> TryFrom<&'a [u8]> for InternalId {
    type Error = anyhow::Error;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if let Ok(b) = <[u8; 16]>::try_from(value) {
            return Ok(InternalId::from(b));
        }
        anyhow::bail!(
            "Invalid InternalId length: Expected 16 bytes but received {}",
            value.len()
        );
    }
}

impl TryFrom<Vec<u8>> for InternalId {
    type Error = anyhow::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from(&value[..])
    }
}
