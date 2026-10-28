use std::{
    ops::Deref,
    sync::Arc,
};

use parking_lot::Mutex;

/// The client pool is a generic that allows us to workaround limitations of
/// individual clients implementations, such as AWS Lambda rust client. It
/// limits the concurrency to each individual client and transparently creates
/// new clients and multiplexes requests. It is currently implemented using a
/// mutex walking over all clients on every get(). Thus it is not recommended if
/// you need to do hundreds of thousands of requests per second or shed load
/// over thousands of clients.
pub struct ClientPool<C> {
    create_client: Arc<dyn Fn() -> C + Send + Sync>,
    max_client_concurrency: usize,

    clients: Arc<Mutex<Vec<Arc<C>>>>,
}

impl<C> ClientPool<C> {
    pub fn new(
        create_client: impl Fn() -> C + Send + Sync + 'static,
        max_client_concurrency: usize,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            max_client_concurrency > 0,
            "max_client_concurrency must be positive"
        );
        Ok(Self {
            create_client: Arc::new(create_client),
            max_client_concurrency,
            clients: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn get(&self) -> BorrowedClient<C> {
        let mut clients = self.clients.lock();
        // Check if any of the existing clients have capacity.
        for client in clients.iter() {
            // The pool holds one reference. The remaining references are from
            // borrowed clients.
            if Arc::strong_count(client) - 1 < self.max_client_concurrency {
                return BorrowedClient {
                    inner: client.clone(),
                };
            }
        }

        // Create a new client.
        let client = Arc::new((self.create_client)());
        clients.push(client.clone());
        BorrowedClient { inner: client }
    }
}

pub struct BorrowedClient<C> {
    inner: Arc<C>,
}

impl<C> Deref for BorrowedClient<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}
