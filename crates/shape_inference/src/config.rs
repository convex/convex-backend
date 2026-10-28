use std::fmt::Debug;

use value::identifier::is_valid_identifier;
/// Static config for the shape inference algorithm. This is useful for tests
/// and updating the shape inference algorithm over time.
pub trait ShapeConfig: Copy + Clone + Debug + Eq + Ord + PartialEq + PartialOrd + 'static {
    const MAX_UNION_LENGTH: usize;
    const MAX_OBJECT_FIELDS: usize;

    fn is_valid_string_literal(s: &str) -> bool;

}

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProdConfig {}

impl ShapeConfig for ProdConfig {
    const MAX_OBJECT_FIELDS: usize = 64;
    const MAX_UNION_LENGTH: usize = 16;

    fn is_valid_string_literal(s: &str) -> bool {
        is_valid_identifier(s)
    }

}
