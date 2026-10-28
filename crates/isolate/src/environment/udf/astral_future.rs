//! Somewhat unsafe helper that allows a non-'static future to be polled by a
//! remote handle on the same thread. This allows efficient recursive async
//! functions.
//!
//! Intended usage:
//! ```ignore
//! let future = async_function(); // some non-'static future
//! let body = std::pin::pin!(AstralBody::new(future));
//! // the projected future is `'static` even though `future` was not!
//! let result = recursive_executor.spawn(unsafe { body.project() }).await?;
//! ```
//!
//! A matching `RecursiveExecutor` is provided. It can also work with
//! `tokio::task::LocalSet`, but that one doesn't interoperate well with

use std::{
    cell::RefCell,
    future::{
        self,
        Future,
    },
    marker::PhantomPinned,
    mem,
    pin::{
        pin,
        Pin,
    },
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};

use anyhow::Context as _;
use futures::{
    stream::FuturesUnordered,
    FutureExt as _,
    Stream,
};
use tokio::sync::oneshot;

/// A reference to a future that returns `T`.
/// The body of the future lives somewhere else.
pub struct AstralFuture<T: 'static> {
    inner: Rc<Inner<T>>,
}

/// The body of a possibly non-'static future `F`.
pub struct AstralBody<F: Future>
where
    F::Output: 'static,
{
    inner: Option<Rc<Inner<F::Output>>>,
    body: F, // pinned & its address is stored in `inner`
    _pinned: PhantomPinned,
}

impl<F: Future> AstralBody<F>
where
    F::Output: 'static,
{
    pub fn new(future: F) -> Self {
        Self {
            inner: None,
            body: future,
            _pinned: PhantomPinned,
        }
    }

    /// Create a handle to this future.
    /// Panics if called more than once.
    ///
    /// The returned future yields `None` if the `AstralBody` is dropped.
    ///
    /// # Safety
    /// The `AstralBody` must not be leaked (e.g. via `mem::forget`). Otherwise
    /// the projection will be able to continue calling `poll()` on the
    /// contained future even after its lifetime has ended.
    pub unsafe fn project(self: Pin<&mut Self>) -> AstralFuture<F::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        assert!(
            this.inner.is_none(),
            "AstralBody::project called more than once"
        );
        let inner = Rc::new(Inner {
            body_ref: RefCell::new(Some(unsafe {
                cast_to_static(Pin::new_unchecked(&mut this.body))
            })),
            waker: RefCell::new(None),
        });
        this.inner = Some(inner.clone());
        AstralFuture { inner }
    }
}

impl<T: 'static> Future for AstralFuture<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        {
            let mut last_waker = self.inner.waker.borrow_mut();
            if !last_waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
                *last_waker = Some(cx.waker().clone());
            }
        }
        {
            let mut body_ref = self.inner.body_ref.borrow_mut();
            if let Some(f) = &mut *body_ref {
                f.as_mut().poll(cx).map(Some)
            } else {
                Poll::Ready(None)
            }
        }
    }
}

struct Inner<T: 'static> {
    body_ref: RefCell<Option<Pin<&'static mut dyn Future<Output = T>>>>,
    waker: RefCell<Option<Waker>>,
}

impl<F: Future> Drop for AstralBody<F> {
    fn drop(&mut self) {
        // For safety, delete the reference to `self.body`.
        // If somehow the future is being polled right now, this will panic with a
        // BorrowMutError.
        if let Some(inner) = self.inner.take() {
            assert!(inner.body_ref.take().is_some());
            if let Some(waker) = inner.waker.take() {
                // Wake up the `AstralFuture` so that it returns `None`.
                waker.wake();
            }
        }
    }
}

unsafe fn cast_to_static<T: ?Sized>(
    ptr: Pin<&'_ mut (dyn Future<Output = T> + '_)>,
) -> Pin<&'static mut (dyn Future<Output = T> + 'static)> {
    unsafe { mem::transmute(ptr) }
}

/// A special-purpose executor for use with AstralFuture.
pub struct RecursiveExecutor<T: 'static> {
    queued: RefCell<Vec<(AstralFuture<T>, oneshot::Sender<Option<T>>)>>,
}

impl<T: 'static> RecursiveExecutor<T> {
    /// Spawns another future `f`.
    pub async fn spawn(&self, f: AstralFuture<T>) -> anyhow::Result<T> {
        let (tx, rx) = oneshot::channel();
        self.queued.borrow_mut().push((f, tx));
        rx.await
            .context("RecursiveExecutor dropped")?
            .context("AstralBody dropped")
    }

    /// Runs a future `f` that is allowed to recurse by calling `spawn()`.
    pub async fn run_until<F2: Future>(&self, f: F2) -> F2::Output {
        let mut f = pin!(f);
        let mut running: FuturesUnordered<_> = FuturesUnordered::new();
        future::poll_fn(|cx| {
            while !running.is_empty() {
                match Pin::new(&mut running).poll_next(cx) {
                    Poll::Ready(Some(())) => continue,
                    Poll::Ready(None) | Poll::Pending => break,
                }
            }
            match f.as_mut().poll(cx) {
                Poll::Ready(r) => return Poll::Ready(r),
                Poll::Pending => (),
            }
            let mut queued = self.queued.borrow_mut();
            if !queued.is_empty() {
                for (fut, tx) in queued.drain(..) {
                    running.push(fut.map(move |r| {
                        _ = tx.send(r);
                    }));
                }
                drop(queued);
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await
    }

    pub fn new() -> Self {
        Self {
            queued: RefCell::new(vec![]),
        }
    }
}
