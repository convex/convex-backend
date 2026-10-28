//! Conversions between Fivetran's API types and the deployment-agnostic
//! streaming export types (which live in `common`, since streaming export is a
//! general-purpose API and Fivetran is only one of its consumers).

pub mod selection;
