use async_trait::async_trait;
use convex_sync_types::{
    ClientMessage,
    Timestamp,
};
use tokio::sync::mpsc;
use url::Url;

use crate::value::Value;

pub mod web_socket_manager;

/// Upon a protocol failure, an explanation of the failure to pass in on
/// reconnect
#[derive(Debug)]
pub struct ReconnectRequest {
    pub reason: ReconnectProtocolReason,
    pub max_observed_timestamp: Option<Timestamp>,
}

pub type ReconnectProtocolReason = String;

pub type ServerMessage = convex_sync_types::ServerMessage<Value>;

#[derive(Debug)]
pub enum ProtocolResponse {
    ServerMessage(ServerMessage),
    Failure,
    /// The protocol handled a reconnect request. Responses after this come
    /// from the new connection.
    ReconnectStarted,
}

#[derive(Debug)]
/// The state of the Convex WebSocket connection
pub enum WebSocketState {
    /// The WebSocket is open and connected
    Connected,
    /// The WebSocket is closed and connecting/reconnecting
    Connecting,
}

#[async_trait]
pub trait SyncProtocol: Send + Sized {
    async fn open(
        ws_url: Url,
        // Unbounded, so the protocol never waits for the client worker, which
        // can itself be waiting on the protocol.
        on_response: mpsc::UnboundedSender<ProtocolResponse>,
        on_state_change: Option<mpsc::Sender<WebSocketState>>,
        client_id: &str,
    ) -> anyhow::Result<Self>;
    async fn send(&mut self, message: ClientMessage) -> anyhow::Result<()>;
    async fn reconnect(&mut self, request: ReconnectRequest);
}
