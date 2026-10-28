//! Common utilities across multiple files.

use std::ops::Deref;

pub use value::utils::{
    display_map,
    display_sequence,
};

#[derive(Clone)]
pub struct ReadOnly<T>(T);

impl<T> ReadOnly<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> Deref for ReadOnly<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// Ensures that we are always running Convex services in UTC.
pub fn ensure_utc() -> anyhow::Result<()> {
    if let Ok(val) = std::env::var("TZ")
        && val != "UTC"
    {
        anyhow::bail!("TZ is set, but Convex requires UTC. Unset TZ to continue.")
    }
    unsafe { std::env::set_var("TZ", "UTC") };

    Ok(())
}
