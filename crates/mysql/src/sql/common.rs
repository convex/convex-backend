//! MySQL SQL-building mechanics shared by persistence versions.

macro_rules! as_table {
    ([$param:ident $(, $rest:ident)*], $e: expr) => {{
        [{
            #[allow(non_upper_case_globals)]
            const $param: bool = false;
            as_table!([$($rest),*], $e)
        }, {
            #[allow(non_upper_case_globals)]
            const $param: bool = true;
            as_table!([$($rest),*], $e)
        }]
    }};
    ([], $e: expr) => { $e };
}
pub(crate) use as_table;

/// Selects a compile-time SQL variant by its boolean parameters.
macro_rules! tableify {
    ([$($param:ident),+], $e: expr) => {{
        as_table!([$($param),*], $e)
            $(
                [$param as usize]
            )*
    }};
    ($param:ident, $e: expr) => {
        tableify!([$param], $e)
    };
}
pub(crate) use tableify;
