/// Autogenerate a tuple wrapper around u64 (eg `struct MemberId(u64)`) with
/// niceties
#[macro_export]
macro_rules! tuple_struct_u64 {
    ($(#[$outer:meta])* $name:ident) => {
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
            derive_more::Display,
            derive_more::FromStr,
            derive_more::From,
            derive_more::Into,
            utoipa::ToSchema,
        )]
        pub struct $name(pub u64);

        impl From<$name> for serde_json::Number {
            fn from(id: $name) -> serde_json::Number {
                id.0.into()
            }
        }
    };
}

#[macro_export]
macro_rules! tuple_struct_string {
    ($(#[$outer:meta])* $name:ident) => {
        #[derive(
            Clone,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
            derive_more::Display,
            derive_more::From,
            derive_more::Deref,
            derive_more::AsRef,
            utoipa::ToSchema,
        )]
        #[from(forward)]
        #[as_ref(forward)]
        $(#[$outer])*
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
