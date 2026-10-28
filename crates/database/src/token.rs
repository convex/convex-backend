//! Externalizable tokens that record the currently-observed state within a
//! transaction.

use std::sync::Arc;

use common::types::Timestamp;
use value::heap_size::HeapSize;
use crate::reads::ReadSet;

/// Serialized representation of [`Token`].
pub type SerializedToken = String;

/// A token is a base64 serializable representation of the current read-state
/// for a transaction. This can be externalized to a user and used to represent
/// current transaction state.
#[derive(Clone, Debug)]
pub struct Token {
    read_set: Arc<ReadSet>,
    ts: Timestamp,
}

impl Token {
    #[allow(unused)]
    #[allow(unused)]
    pub fn new(read_set: Arc<ReadSet>, ts: Timestamp) -> Self {
        Self { read_set, ts }
    }

    pub fn empty(ts: Timestamp) -> Self {
        Self {
            read_set: Arc::new(ReadSet::empty()),
            ts,
        }
    }

    pub fn ts(&self) -> Timestamp {
        self.ts
    }

    pub fn reads(&self) -> &ReadSet {
        &self.read_set
    }

    pub fn reads_owned(&self) -> Arc<ReadSet> {
        self.read_set.clone()
    }

    /// Advance the token's timestamp to a new timestamp.
    pub fn advance_ts(&mut self, ts: Timestamp) {
        assert!(self.ts < ts);
        self.ts = ts;
    }
}

impl HeapSize for Token {
    fn heap_size(&self) -> usize {
        self.read_set.heap_size()
    }
}
