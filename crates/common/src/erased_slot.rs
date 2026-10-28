use std::{
    any::Any,
    sync::SyncView,
};

/// Provides a space to stash a value of any type (as long as it is `Send +
/// 'static`).
///
/// This is useful for dyn-compatible traits that want to return data borrowed
/// from some temporary data structure. The temporary structure can be stashed
/// on the `ErasedSlot`.
pub struct ErasedSlot(Box<dyn Any + Send + Sync>);

impl ErasedSlot {
    #[inline]
    pub fn new() -> Self {
        Self(Box::new(()))
    }

    #[inline]
    pub fn insert<T: Send + 'static>(&mut self, value: T) -> &mut T {
        self.0 = <Box<SyncView<T>>>::new(SyncView::new(value));
        self.0.downcast_mut::<SyncView<T>>().unwrap().as_mut()
    }
}

impl Default for ErasedSlot {
    fn default() -> Self {
        Self::new()
    }
}
