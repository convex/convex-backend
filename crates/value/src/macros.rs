// Patterned off of serde_json::json!

#[macro_export(local_inner_macros)]
macro_rules! val {
    //////////////////////////////////////////////////////////////////////////
    // TT muncher for parsing the inside of an array [...]. Produces a vec![...]
    // of the elements.
    //
    // Must be invoked as: val!(@array [] $($tt)*)
    //////////////////////////////////////////////////////////////////////////

    // Done with trailing comma.
    (@array [$($elems:expr,)*]) => {
        internal_vec![$($elems,)*]
    };

    // Done without trailing comma.
    (@array [$($elems:expr),*]) => {
        internal_vec![$($elems),*]
    };

    // Next element is `null`.
    (@array [$($elems:expr,)*] null $($rest:tt)*) => {
        val!(@array [$($elems,)* val!(null)] $($rest)*)
    };

    // Next element is `true`.
    (@array [$($elems:expr,)*] true $($rest:tt)*) => {
        val!(@array [$($elems,)* val!(true)] $($rest)*)
    };

    // Next element is `false`.
    (@array [$($elems:expr,)*] false $($rest:tt)*) => {
        val!(@array [$($elems,)* val!(false)] $($rest)*)
    };

    // Next element is an array.
    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        val!(@array [$($elems,)* val!([$($array)*])] $($rest)*)
    };

    // Next element is a map.
    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        val!(@array [$($elems,)* val!({$($map)*})] $($rest)*)
    };

    // Next element is an expression followed by comma.
    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        val!(@array [$($elems,)* val!($next),] $($rest)*)
    };

    // Last element is an expression with no trailing comma.
    (@array [$($elems:expr,)*] $last:expr) => {
        val!(@array [$($elems,)* val!($last)])
    };

    // Comma after the most recent element.
    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        val!(@array [$($elems,)*] $($rest)*)
    };

    // Unexpected token after most recent element.
    (@array [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        val_unexpected!($unexpected)
    };

    //////////////////////////////////////////////////////////////////////////
    // TT muncher for parsing the inside of an object {...}. Each entry is
    // inserted into the given map variable.
    //
    // Must be invoked as: val!(@object $map () ($($tt)*) ($($tt)*))
    //
    // We require two copies of the input tokens so that we can match on one
    // copy and trigger errors on the other copy.
    //////////////////////////////////////////////////////////////////////////

    // Done.
    (@object $object:ident () () ()) => {};

    // Insert the current entry followed by trailing comma.
    (@object $object:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        let _ = $object.insert(($($key)+).parse()?, $value);
        val!(@object $object () ($($rest)*) ($($rest)*));
    };

    // Current entry followed by unexpected token.
    (@object $object:ident [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        val_unexpected!($unexpected);
    };

    // Insert the last entry without trailing comma.
    (@object $object:ident [$($key:tt)+] ($value:expr)) => {
        let _ = $object.insert(($($key)+).parse()?, $value);
    };

    // Next value is `null`.
    (@object $object:ident ($($key:tt)+) (=> null $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!(null)) $($rest)*);
    };

    // Next value is `true`.
    (@object $object:ident ($($key:tt)+) (=> true $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!(true)) $($rest)*);
    };

    // Next value is `false`.
    (@object $object:ident ($($key:tt)+) (=> false $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!(false)) $($rest)*);
    };

    // Next value is an array.
    (@object $object:ident ($($key:tt)+) (=> [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!([$($array)*])) $($rest)*);
    };

    // Next value is a map.
    (@object $object:ident ($($key:tt)+) (=> {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!({$($map)*})) $($rest)*);
    };

    // Next value is an expression followed by comma.
    (@object $object:ident ($($key:tt)+) (=> $value:expr , $($rest:tt)*) $copy:tt) => {
        val!(@object $object [$($key)+] (val!($value)) , $($rest)*);
    };

    // Last value is an expression with no trailing comma.
    (@object $object:ident ($($key:tt)+) (=> $value:expr) $copy:tt) => {
        val!(@object $object [$($key)+] (val!($value)));
    };

    // Missing value for last entry. Trigger a reasonable error message.
    (@object $object:ident ($($key:tt)+) (=>) $copy:tt) => {
        // "unexpected end of macro invocation"
        val!();
    };

    // Missing colon and value for last entry. Trigger a reasonable error
    // message.
    (@object $object:ident ($($key:tt)+) () $copy:tt) => {
        // "unexpected end of macro invocation"
        val!();
    };

    // Misplaced colon. Trigger a reasonable error message.
    (@object $object:ident () (=> $($rest:tt)*) ($colon:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `=>`".
        val_unexpected!($colon);
    };

    // Found a comma inside a key. Trigger a reasonable error message.
    (@object $object:ident ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `,`".
        val_unexpected!($comma);
    };

    // Key is fully parenthesized. This avoids clippy double_parens false
    // positives because the parenthesization may be necessary here.
    (@object $object:ident () (($key:expr) => $($rest:tt)*) $copy:tt) => {
        val!(@object $object ($key) (=> $($rest)*) (: $($rest)*));
    };

    // Refuse to absorb colon token into key expression.
    (@object $object:ident ($($key:tt)*) (=> $($unexpected:tt)+) $copy:tt) => {
        val_expect_expr_comma!($($unexpected)+);
    };

    // Munch a token into the current key.
    (@object $object:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        val!(@object $object ($($key)* $tt) ($($rest)*) ($($rest)*));
    };

    //////////////////////////////////////////////////////////////////////////
    // The main implementation.
    //
    // Must be invoked as: val!($($val)+)
    //////////////////////////////////////////////////////////////////////////

    (null) => {
        $crate::ConvexValue::Null
    };

    (true) => {
        $crate::ConvexValue::Boolean(true)
    };

    (false) => {
        $crate::ConvexValue::Boolean(false)
    };

    ([]) => {
        $crate::ConvexValue::Array($crate::ConvexArray::empty())
    };

    ([ $($tt:tt)+ ]) => {
        $crate::ConvexValue::Array(
            $crate::ConvexArray::try_from(
                val!(@array [] $($tt)+)
            )?
        )
    };

    ({}) => {
        $crate::ConvexValue::Object($crate::ConvexObject::empty())
    };

    ({ $($tt:tt)+ }) => {
        $crate::ConvexValue::Object({
            let mut object = std::collections::BTreeMap::new();
            val!(@object object () ($($tt)+) ($($tt)+));
            $crate::ConvexObject::try_from(object)?
        })
    };

    // Any Serialize type: numbers, strings, struct literals, variables etc.
    // Must be below every other rule.
    ($other:expr) => {
        $crate::ConvexValue::try_from($other)?
    };
}

// The val macro above cannot invoke vec directly because it uses
// local_inner_macros. A vec invocation there would resolve to $crate::vec.
// Instead invoke vec here outside of local_inner_macros.
#[macro_export]
macro_rules! internal_vec {
    ($($content:tt)*) => {
        vec![$($content)*]
    };
}

#[macro_export]
macro_rules! val_unexpected {
    () => {};
}

#[macro_export]
macro_rules! val_expect_expr_comma {
    ($e:expr , $($tt:tt)*) => {};
}

#[macro_export(local_inner_macros)]
/// Create an object from field/value pairs, panicking if it isn't a valid
/// object.
macro_rules! obj {
    () => ({
        anyhow::Ok($crate::ConvexObject::empty())
    });
    ( $($tt:tt)+ ) => ({
        let mut object = std::collections::BTreeMap::new();
        val!(@object object () ($($tt)+) ($($tt)+));
        $crate::ConvexObject::try_from(object)
    });
}

#[macro_export]
macro_rules! pending_obj {
    ( $($key:expr => $value:expr),* $(,)? ) => ({
        let mut fields =
            std::collections::BTreeMap::<$crate::FieldName, $crate::PendingValue>::new();
        $(
            fields.insert($key.parse()?, $crate::PendingValue::try_from($value)?);
        )*
        $crate::PendingValue::object(fields)
    });
}
