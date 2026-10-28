use uuid::Uuid;
use value::ConvexValue;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, derive_more::Display, derive_more::FromStr,
)]
pub struct StorageUuid(
    Uuid,
);

impl From<Uuid> for StorageUuid {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl TryFrom<StorageUuid> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(s: StorageUuid) -> anyhow::Result<Self> {
        s.to_string().try_into()
    }
}

impl TryFrom<ConvexValue> for StorageUuid {
    type Error = anyhow::Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::String(s) => Ok(StorageUuid(Uuid::try_parse(&s)?)),
            _ => anyhow::bail!("Can only convert Value::String to StorageUuid"),
        }
    }
}
