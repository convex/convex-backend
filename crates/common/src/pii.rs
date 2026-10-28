use std::fmt;

/// PII is a light wrapper struct that implements Debug by omitting the contents
/// when printing in prod.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PII<T>(pub T);

impl<T> std::ops::Deref for PII<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> PII<T> {
    pub fn into_value(self) -> T {
        self.0
    }
}

impl<T> From<T> for PII<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for PII<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PII({})", std::any::type_name::<T>())
    }
}
