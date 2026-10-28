pub mod nullary_algorithm {
    use std::marker::PhantomData;

    use serde::{
        de::{
            value::StrDeserializer,
            IgnoredAny,
            Visitor,
        },
        ser::SerializeMap,
        Deserialize,
        Deserializer,
        Serialize,
        Serializer,
    };

    struct VisitNullary<'de, T>(PhantomData<(&'de (), T)>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for VisitNullary<'de, T> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or object with a `name` key")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            T::deserialize(StrDeserializer::new(&v.to_ascii_uppercase()))
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            #[derive(Deserialize)]
            enum Field {
                #[serde(rename = "name")]
                Name,
                #[serde(other)]
                Other,
            }
            let mut name = None;
            while let Some(field) = map.next_key::<Field>()? {
                if let Field::Name = field {
                    if name.is_some() {
                        return Err(<A::Error as serde::de::Error>::duplicate_field("name"));
                    }
                    name = Some(map.next_value::<String>()?);
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            let mut name =
                name.ok_or_else(|| <A::Error as serde::de::Error>::missing_field("name"))?;
            name.make_ascii_uppercase();
            T::deserialize(StrDeserializer::new(&name))
        }
    }
    // Used to deserialize algorithms that can be provided as either a plain string
    // "SHA-1" or as an object {"name": "SHA-1"}
    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<T, D::Error> {
        d.deserialize_any(VisitNullary::<'de, T>(PhantomData))
    }

    pub fn serialize<S: Serializer, T: Serialize>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("name", value)?;
        map.end()
    }
}

/// Deserializes an algorithm's `name`, which [algorithm normalization][norm]
/// matches case-insensitively, through the type's [`FromStr`].
///
/// [norm]: https://w3c.github.io/webcrypto/#algorithm-normalization-normalize-an-algorithm
pub mod algorithm_name {
    use std::{
        fmt::Display,
        str::FromStr,
    };

    use serde::{
        de::Error as _,
        Deserialize,
        Deserializer,
    };

    pub fn deserialize<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<T, D::Error>
    where
        T::Err: Display,
    {
        let name = String::deserialize(d)?;
        T::from_str(&name).map_err(D::Error::custom)
    }
}
