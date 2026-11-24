//! Glue names: `<component>__<table>__<tablet>`.
//!
//! The readable part is lowercased for Glue, so it can collide (`Users` vs
//! `users`, component `a/b` vs `a__b`). The tablet is unique per deployment
//! and makes the name unique with no allocation state. It is base32 like
//! developer IDs: lowercase alphanumerics, fixed width.

use anyhow::{
    ensure,
    Context,
    Result,
};
use value::{
    base32,
    InternalId,
    TabletId,
};

use super::SourceTable;

pub(crate) const ROOT_COMPONENT: &str = "app";
/// Glue and Athena limit database and table names to 255 bytes.
pub(crate) const MAX_NAME_BYTES: usize = 255;
/// Base32 of a 16-byte tablet ID.
pub(crate) const TABLET_BYTES: usize = base32::encoded_len(16);
const SEPARATOR: &str = "__";

/// Root component is `app`; `/` becomes `__`.
pub(crate) fn component(source: &SourceTable) -> String {
    if source.component.is_empty() {
        ROOT_COMPONENT.to_owned()
    } else {
        source.component.replace('/', SEPARATOR)
    }
}

/// `app__users`.
pub(crate) fn readable(source: &SourceTable) -> String {
    format!("{}{SEPARATOR}{}", component(source), source.table).to_lowercase()
}

/// Readable part, cut from the front to fit, then `__` and the tablet. The
/// tablet is always the last [`TABLET_BYTES`] bytes.
pub(crate) fn glue_name(source: &SourceTable) -> String {
    let budget = MAX_NAME_BYTES - SEPARATOR.len() - TABLET_BYTES;
    format!(
        "{}{SEPARATOR}{}",
        fit(&readable(source), budget),
        tablet_str(source.tablet)
    )
}

pub(crate) fn tablet_str(tablet: TabletId) -> String {
    base32::encode(&tablet.0 .0)
}

/// Inverse of [`tablet_str`]. Only the canonical spelling parses, so two
/// strings never name one tablet.
pub(crate) fn parse_tablet(s: &str) -> Result<TabletId> {
    let bytes: [u8; 16] = base32::decode(s)
        .context("Tablet is not base32")?
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("Tablet is {} bytes, not 16", bytes.len()))?;
    let tablet = TabletId(InternalId::from(bytes));
    ensure!(
        tablet_str(tablet) == s,
        "Tablet {s:?} is not in canonical form"
    );
    Ok(tablet)
}

pub(crate) fn is_glue_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Cuts `name` from the front to `budget` bytes; the table name is at the end
/// so it survives. A cut landing on a digit gets `t_` over its first two
/// bytes: Athena and Trino need quotes around digit-leading names.
fn fit(name: &str, budget: usize) -> String {
    if name.len() <= budget {
        return name.to_owned();
    }
    // ASCII only, so byte offsets are char offsets.
    let mut cut = name[name.len() - budget..].to_owned();
    if cut.as_bytes()[0].is_ascii_digit() {
        cut.replace_range(..2, "t_");
    }
    cut
}
