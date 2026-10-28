//! As the user adds and removes values from the database, the system tracks
//! [`Shape`]s, a form of inferred type describing the set of values currently
//! present. This shape tracking happens without user intervention, and it
//! continues even if the user enforces a schema.
//!
//! Given a sequence of values, a shape is formally an approximate multiset of
//! these values. Storing all of the values would be far too expensive in memory
//! and update time, so we instead store a succinct representation of the, well,
//! shape of the values. To support adding and removing values
//! from the multiset, [`Shape`] maintains a counter of the number of values in
//! addition to [`ShapeEnum`].

mod dashboard;
pub mod reduced;
pub use self::dashboard::dashboard_shape_json;
