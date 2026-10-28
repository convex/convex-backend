use value::FieldPath;

use crate::common::FieldPath as FieldPathProto;

impl From<FieldPath> for FieldPathProto {
    fn from(n: FieldPath) -> Self {
        FieldPathProto {
            fields: Vec::from(n).into_iter().map(|f| f.into()).collect(),
        }
    }
}

impl TryFrom<FieldPathProto> for FieldPath {
    type Error = anyhow::Error;

    fn try_from(value: FieldPathProto) -> Result<Self, Self::Error> {
        FieldPath::new(
            value
                .fields
                .into_iter()
                .map(|f| f.parse())
                .collect::<anyhow::Result<Vec<_>>>()?,
        )
    }
}
