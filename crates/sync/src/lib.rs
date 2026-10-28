#![recursion_limit = "256"]
#![feature(type_alias_impl_trait)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]

mod metrics;
mod state;
pub mod subscription_reconnect;
pub mod worker;

pub use worker::{
    SyncWorker,
    SyncWorkerConfig,
};

pub type ServerMessage = sync_types::ServerMessage<common::value::JsonPackedValue>;
