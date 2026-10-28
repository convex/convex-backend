//! Documents and document identifiers.
//!
//! This is the authoritative representation of a document within the database.
use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::{
        self,
        Debug,
        Display,
    },
    num::FpCategory,
    ops::{
        Deref,
        DerefMut,
    },
    sync::LazyLock,
    time::Duration,
};

use anyhow::Context;
use errors::ErrorMetadata;
use humansize::{
    FormatSize as _,
    BINARY,
};
use itertools::Itertools;
use packed_value::{
    ByteBuffer,
    OpenedValue,
    PackedValue,
};
use pb::common::{
    DocumentUpdate as DocumentUpdateProto,
    PendingDocument as PendingDocumentProto,
    PendingDocumentUpdate as PendingDocumentUpdateProto,
    ResolvedDocument as ResolvedDocumentProto,
};
use serde::Serialize;
use serde_json::{
    Number,
    Value as JsonValue,
};
pub use value::InternalId;
use value::{
    export::ValueFormat,
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    serde::ConvexSerializable,
    sorting::{
        write_sort_key,
        write_sort_key_or_undefined,
    },
    walk::ConvexValueType,
    ConvexObject,
    ConvexValue,
    FieldName,
    FieldPath,
    IdentifierFieldName,
    InternalDocumentId,
    Namespace,
    PendingValue,
    ResolvedDocumentId,
    TableNumber,
    TabletId,
    MAX_COMMIT_TS,
    VALUE_TOO_LARGE_SHORT_MSG,
};

use crate::{
    floating_point::MAX_EXACT_F64_INT,
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    pii::PII,
    runtime::UnixTimestamp,
    types::Timestamp,
    value::Size,
};

/// The database automatically inserts the assigned document ID as an "_id"
/// field.
pub const ID_FIELD: IdentifierFieldName = IdentifierFieldName::const_new("_id");

pub static ID_FIELD_PATH: LazyLock<FieldPath> =
    LazyLock::new(|| FieldPath::new(vec![ID_FIELD.clone()]).unwrap());

/// The database automatically inserts the creation time in each document with
/// the "_creationTime" field. The timestamp is a Float64 of milliseconds since
/// the Unix epoch.
pub const CREATION_TIME_FIELD: IdentifierFieldName =
    IdentifierFieldName::const_new("_creationTime");

pub static CREATION_TIME_FIELD_PATH: LazyLock<FieldPath> =
    LazyLock::new(|| FieldPath::new(vec![CREATION_TIME_FIELD.clone()]).unwrap());

/// The maximum allowed size for a document stored in a user table. This is the
/// `ConvexValue::size` of the document, including system _id/_creationTime
/// fields.
pub const MAX_USER_SIZE: usize = 1 << 20; // 1MB
pub const MAX_DOCUMENT_NESTING: usize = 16;

// The current Unix timestamp (as of 2022-08-02) in milliseconds is
//
//     1659481438151.257
//
// which is represented as a 64-bit floating point number as
//
//     0    10000100111 1000001001100000110011111100000000010001100001101111
//     sign exponent    mantissa
//
// The distance to the next floating point number is then
//
//     2^(0b10000100111 - 1023) * 2^-52
//     = 0.000244140625 ms
//     = 244.140625 ns
//
// This precision holds until the mantissa is entirely ones:
//
//     0 10000100111 1111111111111111111111111111111111111111111111111111
//     = 2199023255551.9998 ms
//     ~ 17 years in the future
//
// If we increment the exponent by one, we get ~86 years in the future with
// a precision of 488.28125 ns. We'll just go with that to be conservative.
pub const CREATION_TIME_PRECISION: Duration = Duration::from_nanos(500);

#[derive(Clone, Copy)]
pub struct CreationTime {
    ts_ms: f64,
}

impl Ord for CreationTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ts_ms.total_cmp(&other.ts_ms)
    }
}

impl PartialOrd for CreationTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for CreationTime {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for CreationTime {}

impl TryFrom<f64> for CreationTime {
    type Error = anyhow::Error;

    fn try_from(ts_ms: f64) -> anyhow::Result<Self> {
        match ts_ms.classify() {
            FpCategory::Zero | FpCategory::Normal | FpCategory::Subnormal => {
                anyhow::ensure!(!ts_ms.is_sign_negative())
            },
            FpCategory::Infinite | FpCategory::Nan => {
                anyhow::bail!("Invalid creation time {ts_ms}")
            },
        }
        Ok(CreationTime { ts_ms })
    }
}

impl TryFrom<Timestamp> for CreationTime {
    type Error = anyhow::Error;

    fn try_from(ts: Timestamp) -> anyhow::Result<Self> {
        Self::try_from(timestamp_to_ms(ts)?)
    }
}

impl From<CreationTime> for f64 {
    fn from(t: CreationTime) -> f64 {
        t.ts_ms
    }
}

impl From<CreationTime> for ConvexValue {
    fn from(t: CreationTime) -> ConvexValue {
        ConvexValue::from(f64::from(t))
    }
}

pub fn timestamp_to_ms(ts: Timestamp) -> anyhow::Result<f64> {
    let nanos = u64::from(ts);

    // 1e6 nanoseconds in a millisecond.
    let ms_integral = nanos / 1_000_000;
    anyhow::ensure!(ms_integral <= (MAX_EXACT_F64_INT as u64));

    let ms_fractional = ((nanos % 1_000_000) as f64) * 1e-6;
    Ok(ms_integral as f64 + ms_fractional)
}

impl TryFrom<CreationTime> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(t: CreationTime) -> anyhow::Result<JsonValue> {
        let n = Number::from_f64(Into::<f64>::into(t))
            .ok_or_else(|| anyhow::anyhow!("f64 failed to convert to json number."))?;
        Ok(JsonValue::Number(n))
    }
}

impl fmt::Debug for CreationTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.ts_ms)
    }
}

impl CreationTime {
    // CreationTime::ONE is a default for tests. We don't use zero because zero
    // is a likely value that a bug may produce in prod, so it is invalid.
    /// Choose the transaction's initial `_creationTime` in milliseconds so that
    /// `floor(creation_time_ms) * 1_000_000 >= snapshot_ts_ns`.
    /// Queries and mutations use that floor for `Date.now()`, which must be at
    /// least the snapshot timestamp and at most each new document's
    /// `_creationTime`.
    pub fn for_transaction(snapshot_ts: Timestamp, wall_clock: Timestamp) -> anyhow::Result<Self> {
        let snapshot_ceil_ms = u64::from(snapshot_ts).div_ceil(1_000_000) as f64;
        Self::try_from(timestamp_to_ms(wall_clock)?.max(snapshot_ceil_ms))
    }

    pub fn increment(&mut self) -> anyhow::Result<Self> {
        let result = *self;

        let next_float = self.ts_ms.next_up();
        *self = Self::try_from(next_float)?;

        Ok(result)
    }
}

/// Set the UDF timestamp (`Date.now()`) based on the transaction's initial
/// `CreationTime`. This ensures `_creationTime >= Date.now()` as the
/// creation-time cursor advances.
pub fn udf_unix_timestamp(creation_time: CreationTime) -> UnixTimestamp {
    UnixTimestamp::from_millis(f64::from(creation_time).floor() as u64)
}

/// Documents store [`Value`]s.
/// DeveloperDocument is the public-facing document type.
#[derive(Clone, Eq, PartialEq)]
pub struct DeveloperDocument {
    id: DeveloperDocumentId,
    creation_time: CreationTime,
    value: PII<ConvexObject>,
}

impl DeveloperDocument {
    pub fn creation_time(&self) -> CreationTime {
        self.creation_time
    }

    /// The [`InternalId`] associated with the document's ID.
    pub fn internal_id(&self) -> InternalId {
        self.id.internal_id()
    }

    /// The body/payload of the document
    pub fn value(&self) -> &PII<ConvexObject> {
        &self.value
    }

    /// Consume the document's value.
    pub fn into_value(self) -> PII<ConvexObject> {
        self.value
    }

    pub fn size(&self) -> usize {
        self.value.size()
    }

    pub fn to_internal_json(&self) -> JsonValue {
        self.value.0.to_internal_json()
    }

    pub fn to_internal_json_serializable(&self) -> impl Serialize + '_ {
        self.value.0.to_internal_json_serializable()
    }
}

impl Debug for DeveloperDocument {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Document({:?})", self.value)
    }
}

impl Display for DeveloperDocument {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Document(value: {})", self.value.0)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedDocument {
    tablet_id: TabletId,
    document: DeveloperDocument,
}

impl Deref for ResolvedDocument {
    type Target = DeveloperDocument;

    fn deref(&self) -> &Self::Target {
        &self.document
    }
}

impl Debug for ResolvedDocument {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.document, f)
    }
}

impl Display for ResolvedDocument {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.document, f)
    }
}

impl TryFrom<ResolvedDocument> for ResolvedDocumentProto {
    type Error = anyhow::Error;

    fn try_from(
        ResolvedDocument {
            tablet_id,
            document:
                DeveloperDocument {
                    id,
                    creation_time,
                    value,
                },
        }: ResolvedDocument,
    ) -> anyhow::Result<Self> {
        let value = value.0.json_serialize()?.into_bytes();
        let id = ResolvedDocumentId {
            tablet_id,
            developer_id: id,
        };
        Ok(Self {
            id: Some(id.into()),
            creation_time: Some(creation_time.into()),
            value: Some(value),
        })
    }
}

impl TryFrom<ResolvedDocumentProto> for ResolvedDocument {
    type Error = anyhow::Error;

    fn try_from(
        ResolvedDocumentProto {
            id,
            creation_time,
            value,
        }: ResolvedDocumentProto,
    ) -> anyhow::Result<Self> {
        let id: ResolvedDocumentId = id
            .ok_or_else(|| anyhow::anyhow!("Missing id"))?
            .try_into()?;
        let creation_time = creation_time
            .ok_or_else(|| anyhow::anyhow!("Missing creation time"))?
            .try_into()?;
        let value: ConvexObject = serde_json::from_slice::<JsonValue>(
            &value.ok_or_else(|| anyhow::anyhow!("Missing value"))?,
        )?
        .try_into()?;

        Ok(Self {
            tablet_id: id.tablet_id,
            document: DeveloperDocument {
                id: id.into(),
                creation_time,
                value: PII(value),
            },
        })
    }
}

impl ResolvedDocument {
    pub fn new(
        id: ResolvedDocumentId,
        creation_time: CreationTime,
        mut value: ConvexObject,
    ) -> anyhow::Result<Self> {
        let id_value: ConvexValue = id.into();
        if let Some(existing_value) = value.get(&FieldName::from(ID_FIELD)) {
            if existing_value != &id_value {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidIdError",
                    format!(
                        "Provided document ID {id_value} doesn't match '_id' field \
                         {existing_value}"
                    ),
                ));
            }
        } else {
            value = value.insert(ID_FIELD.into(), id_value)?;
        }
        let creation_time_value = ConvexValue::from(f64::from(creation_time));
        match (
            creation_time_value,
            value.get(&FieldName::from(CREATION_TIME_FIELD)),
        ) {
            (time, None) => {
                value = value.insert(CREATION_TIME_FIELD.into(), time)?;
            },
            (l, r) if Some(&l) == r => (),
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidCreationTimeError",
                format!(
                    "Provided creation time {creation_time:?} doesn't match '_creationTime' field \
                     in {value}"
                ),
            )),
        }
        let doc = Self {
            tablet_id: id.tablet_id,
            document: DeveloperDocument {
                id: id.into(),
                creation_time,
                value: PII(value),
            },
        };
        doc.must_validate()?;
        Ok(doc)
    }

    fn must_validate(&self) -> anyhow::Result<()> {
        let violations = self.validate();
        if !violations.is_empty() {
            let msg = format!(
                "{self} isn't a valid document: {}",
                violations.into_iter().map(|v| v.to_string()).join("\n ")
            );
            anyhow::bail!(ErrorMetadata::bad_request("InvalidDocumentError", msg));
        }
        Ok(())
    }

    /// Checks system fields _id and _creationTime and checks that there aren't
    /// any other top-level system fields.
    /// Returns vec of violations, which may be displayed to clients.
    pub fn validate(&self) -> Vec<DocumentValidationError> {
        let mut violations = vec![];

        let nesting = self.value().nesting();
        if nesting > MAX_DOCUMENT_NESTING {
            violations.push(DocumentValidationError::TooNested(nesting));
        }

        match self.value.get(&FieldName::from(ID_FIELD.clone())) {
            Some(ConvexValue::String(s)) => {
                if let Ok(document_id) = DeveloperDocumentId::decode(s) {
                    if document_id.table() != self.id.table() {
                        violations.push(DocumentValidationError::IdWrongTable);
                    } else if document_id.internal_id() != self.internal_id() {
                        violations.push(DocumentValidationError::IdMismatch(
                            self.id,
                            ConvexValue::String(s.clone()),
                        ));
                    }
                }
            },
            Some(v) => violations.push(DocumentValidationError::IdBadType(v.clone())),
            None => violations.push(DocumentValidationError::IdMissing),
        }
        match self
            .value
            .get(&FieldName::from(CREATION_TIME_FIELD.clone()))
        {
            Some(creation_time_value) => {
                if let ConvexValue::Float64(creation_time) = creation_time_value {
                    // Intentionally exclude -0, -inf, inf, and NaN.
                    if !(*creation_time > 0.0 && *creation_time < MAX_EXACT_F64_INT as f64) {
                        violations.push(DocumentValidationError::CreationTimeInvalidFloat(
                            *creation_time,
                        ));
                    }
                } else {
                    violations.push(DocumentValidationError::CreationTimeBadType(
                        creation_time_value.clone(),
                    ));
                }
            },
            None => violations.push(DocumentValidationError::CreationTimeMissing),
        }
        for (field, _) in self.value.iter() {
            if field == &ID_FIELD.into() || field == &CREATION_TIME_FIELD.into() {
                continue;
            }
            if field.is_system() {
                violations.push(DocumentValidationError::SystemField(field.clone()));
            }
        }
        violations
    }

    /// Returns the set of values that this document should be indexed by for
    /// the given fields if they exist in the document
    pub fn index_key(&self, fields: &[FieldPath]) -> IndexKey {
        let mut values = vec![];
        for field in fields.iter() {
            if let Some(v) = self.value.get_path(field) {
                values.push(Some(v.clone()));
            } else {
                values.push(None);
            }
        }
        IndexKey::new_allow_missing(values, self.developer_id())
    }

    /// Recreate a `Document` from an already-written value to the database.
    /// This method assumes that system-provided fields, like `_id`, have
    /// already been inserted into `value`.
    pub fn from_database(tablet_id: TabletId, value: ConvexValue) -> anyhow::Result<Self> {
        let object: ConvexObject = value.try_into()?;
        let id = match object.get(&FieldName::from(ID_FIELD.clone())) {
            Some(ConvexValue::String(s)) => DeveloperDocumentId::decode(s)?,
            _ => anyhow::bail!("Object {} missing _id field", object),
        };
        let creation_time = match object.get(&FieldName::from(CREATION_TIME_FIELD.clone())) {
            Some(ConvexValue::Float64(ts)) => (*ts).try_into()?,
            None => anyhow::bail!("Object {object} missing _creationTime field"),
            _ => anyhow::bail!("Object {object} has invalid _creationTime field"),
        };
        Ok(Self {
            tablet_id,
            document: DeveloperDocument {
                id,
                creation_time,
                value: PII(object),
            },
        })
    }

    pub fn from_packed(
        value: ConvexValue,
        document_id: ResolvedDocumentId,
    ) -> anyhow::Result<Self> {
        let object: ConvexObject = value.try_into()?;
        let creation_time = match object.get(&FieldName::from(CREATION_TIME_FIELD.clone())) {
            Some(ConvexValue::Float64(ts)) => (*ts).try_into()?,
            None => anyhow::bail!("Object {object} missing _creationTime field"),
            _ => anyhow::bail!("Object {object} has invalid _creationTime field"),
        };
        Ok(Self {
            tablet_id: document_id.tablet_id,
            document: DeveloperDocument {
                id: document_id.into(),
                creation_time,
                value: PII(object),
            },
        })
    }

    /// Return a new [`Document`] with the value updated to `new_value`. If
    /// `new_value` contains an `_id` field, it must match the current `_id`
    /// field's value.
    pub fn replace_value(&self, new_value: ConvexObject) -> anyhow::Result<Self> {
        Self::new(self.id(), self.creation_time, new_value)
    }

    pub fn into_value(self) -> PII<ConvexObject> {
        self.document.into_value()
    }

    pub fn to_developer(self) -> DeveloperDocument {
        self.document
    }

    pub fn id_with_table_id(&self) -> InternalDocumentId {
        InternalDocumentId::new(self.tablet_id, self.id.internal_id())
    }

    pub fn id(&self) -> ResolvedDocumentId {
        ResolvedDocumentId::new(self.tablet_id, self.id)
    }

    pub fn developer_id(&self) -> DeveloperDocumentId {
        self.id
    }

    pub fn export(self, format: ValueFormat) -> JsonValue {
        self.document.into_value().0.export(format)
    }

    /// Enforce that the size of the underlying ConvexObject doesn't exceed
    /// `MAX_USER_SIZE`.
    pub fn check_user_size(&self) -> anyhow::Result<()> {
        let size = self.value.size();
        if size > MAX_USER_SIZE {
            anyhow::bail!(ErrorMetadata::bad_request(
                VALUE_TOO_LARGE_SHORT_MSG,
                format!(
                    "Value is too large ({} > maximum size {})",
                    size.format_size(BINARY),
                    MAX_USER_SIZE.format_size(BINARY),
                )
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentUpdateWithPrevTs {
    pub id: ResolvedDocumentId,
    /// The old document and its timestamp in the document log.
    /// The timestamp will become the update's `prev_ts`.
    pub old_document: Option<(ResolvedDocument, Timestamp)>,
    pub new_document: Option<ResolvedDocument>,
}

/// A new document staged in a transaction whose body may contain commit
/// timestamps that are unknown until the transaction commits (see
/// [`PendingValue`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingDocument {
    Concrete(ResolvedDocument),
    Pending {
        id: ResolvedDocumentId,
        creation_time: CreationTime,
        body: PendingValue,
    },
}

impl From<ResolvedDocument> for PendingDocument {
    fn from(document: ResolvedDocument) -> Self {
        Self::Concrete(document)
    }
}

impl PendingDocument {
    pub fn new(
        id: ResolvedDocumentId,
        creation_time: CreationTime,
        body: PendingValue,
    ) -> anyhow::Result<Self> {
        match body {
            PendingValue::Concrete(value) => {
                let document = ResolvedDocument::new(id, creation_time, document_object(value)?)?;
                Ok(document.into())
            },
            PendingValue::Object { mut fields, .. } => {
                // The body carries the system fields, exactly like a
                // materialized document's value. If the body already has
                // them, they must match `id` and `creation_time`.
                for (field, value) in [
                    (ID_FIELD, ConvexValue::from(id)),
                    (CREATION_TIME_FIELD, ConvexValue::from(creation_time)),
                ] {
                    let value = PendingValue::Concrete(value);
                    if let Some(existing) = fields.insert(field.clone().into(), value.clone()) {
                        anyhow::ensure!(
                            existing == value,
                            ErrorMetadata::bad_request(
                                "InvalidDocumentError",
                                format!("Document's {field} field doesn't match"),
                            )
                        );
                    }
                }
                Ok(Self::Pending {
                    id,
                    creation_time,
                    body: PendingValue::object(fields)?,
                })
            },
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidDocumentError",
                "Documents must be objects",
            )),
        }
    }

    pub fn id(&self) -> ResolvedDocumentId {
        match self {
            Self::Concrete(document) => document.id(),
            Self::Pending { id, .. } => *id,
        }
    }

    pub fn creation_time(&self) -> CreationTime {
        match self {
            Self::Concrete(document) => document.creation_time(),
            Self::Pending { creation_time, .. } => *creation_time,
        }
    }

    /// The document, if it contains no unresolved commit timestamps.
    pub fn as_concrete(&self) -> Option<&ResolvedDocument> {
        match self {
            Self::Concrete(document) => Some(document),
            Self::Pending { .. } => None,
        }
    }

    /// The document's pre-commit view, with unresolved commit timestamps set to
    /// `Int64(i64::MAX)`
    pub fn to_document_with_max_commit_ts(&self) -> anyhow::Result<Cow<'_, ResolvedDocument>> {
        match self {
            Self::Concrete(document) => Ok(Cow::Borrowed(document)),
            Self::Pending {
                id,
                creation_time,
                body,
            } => Ok(Cow::Owned(ResolvedDocument::new(
                *id,
                *creation_time,
                document_object(body.resolve(MAX_COMMIT_TS)?.into_owned())?,
            )?)),
        }
    }

    /// The document's body (including its system fields), e.g. to merge a
    /// patch into it.
    pub fn into_pending_value(self) -> PendingValue {
        match self {
            Self::Concrete(document) => {
                ConvexValue::Object(document.into_value().into_value()).into()
            },
            Self::Pending { body, .. } => body,
        }
    }

    /// Size and nesting of the eventual resolved document.
    pub fn size_and_nesting(&self) -> (usize, usize) {
        match self {
            Self::Concrete(document) => (document.size(), document.value().nesting()),
            Self::Pending { body, .. } => (body.size(), body.nesting()),
        }
    }

    /// Internal JSON with `{"$commitTs": null}` at each unresolved commit
    /// timestamp.
    pub fn to_uncommitted_internal_json(&self) -> impl Serialize + '_ {
        struct UncommittedJsonDocument<'a>(&'a PendingDocument);
        impl<'a> Serialize for UncommittedJsonDocument<'a> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                match self.0 {
                    PendingDocument::Concrete(document) => document
                        .value()
                        .to_internal_json_serializable()
                        .serialize(serializer),
                    PendingDocument::Pending { body, .. } => body
                        .to_uncommitted_json_serializable()
                        .serialize(serializer),
                }
            }
        }
        UncommittedJsonDocument(self)
    }

    // Must be a document that does not contain unresolved commit timestamps
    pub fn into_resolved(self) -> anyhow::Result<ResolvedDocument> {
        match self {
            Self::Concrete(document) => Ok(document),
            Self::Pending { .. } => {
                anyhow::bail!("Document contains an unresolved commit timestamp")
            },
        }
    }

    /// Replace each unresolved commit timestamp with `commit_ts`. Concrete
    /// documents are returned without cloning.
    pub fn resolve(self, commit_ts: i64) -> anyhow::Result<ResolvedDocument> {
        match self {
            Self::Concrete(document) => Ok(document),
            Self::Pending {
                id,
                creation_time,
                body,
            } => ResolvedDocument::new(
                id,
                creation_time,
                document_object(body.into_resolved(commit_ts)?)?,
            ),
        }
    }
}

fn document_object(value: ConvexValue) -> anyhow::Result<ConvexObject> {
    let ConvexValue::Object(object) = value else {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidDocumentError",
            "Documents must be objects",
        ));
    };
    Ok(object)
}

/// A single document's update in a transaction's write set. The new document
/// may contain unresolved commit timestamps; [`PendingDocumentUpdate::resolve`]
/// produces the final [`DocumentUpdateWithPrevTs`] once the commit timestamp
/// is known, borrowing when there is nothing to resolve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingDocumentUpdate {
    pub id: ResolvedDocumentId,
    pub old_document: Option<(ResolvedDocument, Timestamp)>,
    pub new_document: Option<PendingDocument>,
}

impl From<DocumentUpdateWithPrevTs> for PendingDocumentUpdate {
    fn from(update: DocumentUpdateWithPrevTs) -> Self {
        Self {
            id: update.id,
            old_document: update.old_document,
            new_document: update.new_document.map(PendingDocument::from),
        }
    }
}

impl PendingDocumentUpdate {
    pub fn new(
        id: ResolvedDocumentId,
        old_document: Option<(ResolvedDocument, Timestamp)>,
        new_document: Option<PendingDocument>,
    ) -> Self {
        if let Some(document) = &new_document {
            debug_assert_eq!(id, document.id());
        }
        Self {
            id,
            old_document,
            new_document,
        }
    }

    pub fn id(&self) -> ResolvedDocumentId {
        self.id
    }

    pub fn id_ref(&self) -> &ResolvedDocumentId {
        &self.id
    }

    pub fn old_document(&self) -> Option<&(ResolvedDocument, Timestamp)> {
        self.old_document.as_ref()
    }

    /// The new document's pre-commit view, with unresolved commit timestamps
    /// at `Int64(i64::MAX)`
    pub fn new_document_with_max_commit_ts(
        &self,
    ) -> anyhow::Result<Option<Cow<'_, ResolvedDocument>>> {
        self.new_document
            .as_ref()
            .map(|document| document.to_document_with_max_commit_ts())
            .transpose()
    }

    pub fn new_document_size_and_nesting(&self) -> Option<(usize, usize)> {
        self.new_document
            .as_ref()
            .map(PendingDocument::size_and_nesting)
    }

    /// Whether this update contains no unresolved commit timestamps.
    pub fn is_resolved(&self) -> bool {
        !matches!(self.new_document, Some(PendingDocument::Pending { .. }))
    }

    pub fn into_resolved(self) -> anyhow::Result<DocumentUpdateWithPrevTs> {
        Ok(DocumentUpdateWithPrevTs {
            id: self.id,
            old_document: self.old_document,
            new_document: self
                .new_document
                .map(PendingDocument::into_resolved)
                .transpose()?,
        })
    }

    /// Replace each unresolved commit timestamp with `commit_ts`. Concrete
    /// documents are moved through without cloning.
    pub fn resolve(self, commit_ts: Timestamp) -> anyhow::Result<DocumentUpdateWithPrevTs> {
        Ok(DocumentUpdateWithPrevTs {
            id: self.id,
            old_document: self.old_document,
            new_document: self
                .new_document
                .map(|document| document.resolve(i64::from(commit_ts)))
                .transpose()?,
        })
    }

    /// Internal JSON of the new document, with `{"$commitTs": null}` at each
    /// unresolved commit timestamp.
    pub fn new_document_internal_json(&self) -> Option<impl Serialize + '_> {
        self.new_document
            .as_ref()
            .map(PendingDocument::to_uncommitted_internal_json)
    }
}

impl TryFrom<PendingDocument> for PendingDocumentProto {
    type Error = anyhow::Error;

    fn try_from(document: PendingDocument) -> anyhow::Result<Self> {
        let value = serde_json::to_vec(&document.to_uncommitted_internal_json())?;
        Ok(Self {
            id: Some(document.id().into()),
            creation_time: Some(document.creation_time().into()),
            value: Some(value),
        })
    }
}

impl TryFrom<PendingDocumentProto> for PendingDocument {
    type Error = anyhow::Error;

    fn try_from(
        PendingDocumentProto {
            id,
            creation_time,
            value,
        }: PendingDocumentProto,
    ) -> anyhow::Result<Self> {
        let id: ResolvedDocumentId = id.context("Missing id")?.try_into()?;
        let creation_time = creation_time.context("Missing creation time")?.try_into()?;
        let body = PendingValue::from_uncommitted_json(serde_json::from_slice(
            &value.context("Missing value")?,
        )?)?;
        Self::new(id, creation_time, body)
    }
}

impl TryFrom<PendingDocumentUpdate> for PendingDocumentUpdateProto {
    type Error = anyhow::Error;

    fn try_from(
        PendingDocumentUpdate {
            id,
            old_document,
            new_document,
        }: PendingDocumentUpdate,
    ) -> anyhow::Result<Self> {
        let (old_document, old_ts) = old_document.unzip();
        Ok(Self {
            id: Some(id.into()),
            old_document: old_document.map(|d| d.try_into()).transpose()?,
            old_ts: old_ts.map(|ts| ts.into()),
            new_document: new_document.map(|d| d.try_into()).transpose()?,
        })
    }
}

impl TryFrom<PendingDocumentUpdateProto> for PendingDocumentUpdate {
    type Error = anyhow::Error;

    fn try_from(
        PendingDocumentUpdateProto {
            id,
            old_document,
            old_ts,
            new_document,
        }: PendingDocumentUpdateProto,
    ) -> anyhow::Result<Self> {
        let id = id
            .context("Document updates missing document id")?
            .try_into()?;
        Ok(Self {
            id,
            old_document: old_document
                .map(|d| {
                    anyhow::Ok((
                        d.try_into()?,
                        Timestamp::try_from(old_ts.context("old_ts missing")?)?,
                    ))
                })
                .transpose()?,
            new_document: new_document.map(|d| d.try_into()).transpose()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentUpdate {
    pub id: ResolvedDocumentId,
    pub old_document: Option<ResolvedDocument>,
    pub new_document: Option<ResolvedDocument>,
}

impl TryFrom<DocumentUpdate> for DocumentUpdateProto {
    type Error = anyhow::Error;

    fn try_from(
        DocumentUpdate {
            id,
            old_document,
            new_document,
        }: DocumentUpdate,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: Some(id.into()),
            old_document: old_document.map(|d| d.try_into()).transpose()?,
            new_document: new_document.map(|d| d.try_into()).transpose()?,
        })
    }
}

impl TryFrom<DocumentUpdateProto> for DocumentUpdate {
    type Error = anyhow::Error;

    fn try_from(
        DocumentUpdateProto {
            id,
            old_document,
            new_document,
        }: DocumentUpdateProto,
    ) -> anyhow::Result<Self> {
        let id = id
            .context("Document updates missing document id")?
            .try_into()?;
        Ok(Self {
            id,
            old_document: old_document.map(|d| d.try_into()).transpose()?,
            new_document: new_document.map(|d| d.try_into()).transpose()?,
        })
    }
}

impl From<DocumentUpdateWithPrevTs> for DocumentUpdate {
    fn from(update: DocumentUpdateWithPrevTs) -> Self {
        Self {
            id: update.id,
            old_document: update.old_document.map(|(d, _)| d),
            new_document: update.new_document,
        }
    }
}

/// Either a [`DocumentUpdate`] or a [`DocumentUpdateWithPrevTs`]
pub trait DocumentUpdateRef {
    fn id(&self) -> ResolvedDocumentId;
    fn old_document(&self) -> Option<&ResolvedDocument>;
    fn new_document(&self) -> Option<&ResolvedDocument>;
}

impl DocumentUpdateRef for DocumentUpdateWithPrevTs {
    fn id(&self) -> ResolvedDocumentId {
        self.id
    }

    fn old_document(&self) -> Option<&ResolvedDocument> {
        self.old_document.as_ref().map(|(d, _)| d)
    }

    fn new_document(&self) -> Option<&ResolvedDocument> {
        self.new_document.as_ref()
    }
}

impl DocumentUpdateRef for DocumentUpdate {
    fn id(&self) -> ResolvedDocumentId {
        self.id
    }

    fn old_document(&self) -> Option<&ResolvedDocument> {
        self.old_document.as_ref()
    }

    fn new_document(&self) -> Option<&ResolvedDocument> {
        self.new_document.as_ref()
    }
}

impl DeveloperDocument {
    pub fn new(id: DeveloperDocumentId, creation_time: CreationTime, value: ConvexObject) -> Self {
        Self {
            id,
            creation_time,
            value: PII(value),
        }
    }

    pub fn to_resolved(self, tablet_id: TabletId) -> ResolvedDocument {
        ResolvedDocument {
            tablet_id,
            document: self,
        }
    }

    pub fn id(&self) -> DeveloperDocumentId {
        self.id
    }

    pub fn table(&self) -> TableNumber {
        self.id.table()
    }
}

/// Two packed values, the actual document value and the document ID. The
/// document ID will contain more information that the `_id` field of the value
/// when we are using ID strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedDocument {
    value: PackedValue<ByteBuffer>,
    id: ResolvedDocumentId,
    /// The `ConvexValue` size of the document, as computed at pack time.
    /// This matches `ResolvedDocument::size()`.
    size: usize,
}

impl PackedDocument {
    pub fn pack(document: &ResolvedDocument) -> Self {
        let id = document.id();
        let size = document.size();
        let value = PackedValue::pack_object(document.document.value());
        Self { value, id, size }
    }

    pub fn from_raw(bytes: Vec<u8>, id: ResolvedDocumentId, size: usize) -> Self {
        let value = PackedValue::new(ByteBuffer::from(bytes));
        Self { value, id, size }
    }

    pub fn shrink(self) -> Self {
        Self {
            value: self.value.shrink(),
            id: self.id,
            size: self.size,
        }
    }

    pub fn unpack(&self) -> ResolvedDocument {
        let value =
            ConvexValue::try_from(self.value.as_ref()).expect("Couldn't unpack packed value");
        ResolvedDocument::from_packed(value, self.id)
            .expect("Packed value wasn't a valid document?")
    }

    /// Same behavior as ResolvedDocument::id but you don't have to fully
    /// unpack.
    pub fn id(&self) -> ResolvedDocumentId {
        self.id
    }

    pub fn developer_id(&self) -> DeveloperDocumentId {
        self.id().developer_id
    }

    pub fn value(&self) -> &PackedValue<ByteBuffer> {
        &self.value
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Like ResolvedDocument::index_key().into_bytes(), but you don't have to
    /// fully unpack.
    ///
    /// `buffer` is an existing allocation that will be cleared and reused.
    pub fn index_key<'a>(
        &self,
        fields: &[FieldPath],
        buffer: &'a mut IndexKeyBuffer,
    ) -> &'a IndexKeyBytes {
        let out = &mut buffer.0 .0;
        out.clear();
        for field_path in fields {
            let value = self.value.as_ref().open_path(field_path);
            write_sort_key_or_undefined(value, out).expect("failed to unpack opened value");
        }
        let mut id_buffer = Default::default();
        let id = self.id().developer_id.encode_into(&mut id_buffer);
        // use `reserve_exact` for memory efficiency
        out.reserve_exact(id.len() + 2);
        let Ok(()) = write_sort_key(id, out);
        &buffer.0
    }

    pub fn index_key_owned(&self, fields: &[FieldPath]) -> IndexKeyBytes {
        let mut buffer = IndexKeyBuffer::new();
        self.index_key(fields, &mut buffer);
        buffer.0
    }
}

/// A reusable allocation for use by `PackedDocument::index_key`
pub struct IndexKeyBuffer(IndexKeyBytes);
impl IndexKeyBuffer {
    pub fn new() -> Self {
        Self(IndexKeyBytes(Vec::new()))
    }
}

impl HeapSize for PackedDocument {
    fn heap_size(&self) -> usize {
        self.value.heap_size() + self.id.heap_size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument<D> {
    id: ResolvedDocumentId,
    creation_time: CreationTime,
    value: D,
}

impl<D> ParsedDocument<D> {
    pub fn id(&self) -> ResolvedDocumentId {
        self.id
    }

    pub fn developer_id(&self) -> DeveloperDocumentId {
        self.id.into()
    }

    pub fn creation_time(&self) -> CreationTime {
        self.creation_time
    }

    pub fn into_value(self) -> D {
        self.value
    }

    pub fn into_id_and_value(self) -> (ResolvedDocumentId, D) {
        (self.id, self.value)
    }

    pub fn map<E>(
        self,
        f: impl FnOnce(D) -> anyhow::Result<E>,
    ) -> anyhow::Result<ParsedDocument<E>> {
        Ok(ParsedDocument {
            id: self.id,
            creation_time: self.creation_time,
            value: f(self.value)?,
        })
    }
}

pub trait ParseDocument<D> {
    fn parse(self) -> anyhow::Result<ParsedDocument<D>>;
}

// this impl can't use ConvexSerializable because it's used with some types that
// directly impl `From<ConvexObject>`
impl<D> ParseDocument<D> for ResolvedDocument
where
    D: TryFrom<ConvexObject, Error = anyhow::Error>,
{
    fn parse(self) -> anyhow::Result<ParsedDocument<D>> {
        let id = self.id();
        let creation_time = self.creation_time;
        let value: D = self
            .document
            .into_value()
            .0
            .try_into()
            .with_context(|| format!("Failed to parse document id: {id}"))?;
        Ok(ParsedDocument {
            id,
            creation_time,
            value,
        })
    }
}

impl<D: ConvexSerializable> ParseDocument<D> for &ResolvedDocument {
    fn parse(self) -> anyhow::Result<ParsedDocument<D>> {
        let id = self.id();
        let creation_time = self.creation_time;
        let value: D = value::serde::from_value::<_, D::Serialized>(
            ConvexValueType::<&ConvexValue>::Object(&self.document.value().0),
        )?
        .try_into()
        .map_err(Into::<anyhow::Error>::into)
        .with_context(|| format!("Failed to parse document id: {id}"))?;
        Ok(ParsedDocument {
            id,
            creation_time,
            value,
        })
    }
}

impl<D: ConvexSerializable> ParseDocument<D> for &PackedDocument {
    fn parse(self) -> anyhow::Result<ParsedDocument<D>> {
        let creation_time = match self.value().as_ref().open()? {
            OpenedValue::Object(o) => match o.get(&CREATION_TIME_FIELD)? {
                Some(OpenedValue::Float64(ts)) => CreationTime::try_from(ts)?,
                None => anyhow::bail!("PackedDocument missing _creationTime field"),
                _ => anyhow::bail!("PackedDocument has non-float64 _creationTime field"),
            },
            v => anyhow::bail!("PackedDocument is {v:?}, not object"),
        };
        Ok(ParsedDocument {
            id: self.id,
            creation_time,
            value: self.value.parse()?,
        })
    }
}

impl<D: ConvexSerializable> ParseDocument<D> for PackedDocument {
    fn parse(self) -> anyhow::Result<ParsedDocument<D>> {
        (&self).parse()
    }
}

impl<D> ParseDocument<D> for ParsedDocument<D> {
    fn parse(self) -> anyhow::Result<ParsedDocument<D>> {
        Ok(self)
    }
}

impl<D> Deref for ParsedDocument<D> {
    type Target = D;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<D> DerefMut for ParsedDocument<D> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentValidationError {
    #[error(
        "Field '{0}' starts with an underscore, which is only allowed for system fields like '_id'"
    )]
    SystemField(FieldName),
    #[error("The document belongs to a different table than its '_id' field")]
    IdWrongTable,
    #[error("The document has id {0}, but its '_id' field is {1}")]
    IdMismatch(DeveloperDocumentId, ConvexValue),
    #[error("The '_id' field {0} must be an Id")]
    IdBadType(ConvexValue),
    #[error("The '_id' field is missing")]
    IdMissing,
    #[error("The '_creationTime' field should be a timestamp in milliseconds, is {0}")]
    CreationTimeInvalidFloat(f64),
    #[error("The '_creationTime' field should be a float, is {0}")]
    CreationTimeBadType(ConvexValue),
    #[error("The '_creationTime' field is missing")]
    CreationTimeMissing,
    #[error(
        "Document is too nested (nested {0} levels deep > maximum nesting {MAX_DOCUMENT_NESTING})"
    )]
    TooNested(usize),
}

impl DocumentValidationError {
    pub fn violation(&self) -> &'static str {
        match self {
            DocumentValidationError::SystemField(_) => "invalid system field",
            DocumentValidationError::IdWrongTable => "_id wrong table",
            DocumentValidationError::IdMismatch(..) => "_id mismatch",
            DocumentValidationError::IdBadType(_) => "_id is not an Id",
            DocumentValidationError::IdMissing => "_id missing",
            DocumentValidationError::CreationTimeInvalidFloat(_) => "_creationTime invalid float",
            DocumentValidationError::CreationTimeBadType(_) => "_creationTime wrong type",
            DocumentValidationError::CreationTimeMissing => "_creationTime missing",
            DocumentValidationError::TooNested(_) => "too nested",
        }
    }
}
