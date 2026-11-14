//! Tests of [`ConvexClient`] against a WebSocket server in the same process,
//! so they exercise the real WebSocket worker along with the client worker.
use std::{
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use convex_sync_types::{
    AuthenticationToken,
    ClientMessage,
    LogLinesMessage,
    QueryId,
    QuerySetModification,
    QuerySetVersion,
    StateModification,
    StateVersion,
    Timestamp,
};
use futures::{
    SinkExt,
    StreamExt,
};
use maplit::btreemap;
use serde_json::Value as JsonValue;
use tokio::{
    net::{
        TcpListener,
        TcpStream,
    },
    sync::Notify,
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::Message,
    WebSocketStream,
};

use crate::{
    sync::ServerMessage,
    ConvexClient,
    FunctionResult,
    Value,
};

const TIMEOUT: Duration = Duration::from_secs(5);

struct FakeServer {
    listener: TcpListener,
}

impl FakeServer {
    async fn start() -> anyhow::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind("127.0.0.1:0").await?,
        })
    }

    fn url(&self) -> anyhow::Result<String> {
        Ok(format!("http://{}", self.listener.local_addr()?))
    }

    /// Waits for the client's next connection.
    async fn accept(&self) -> anyhow::Result<FakeConnection> {
        let (stream, _) = tokio::time::timeout(TIMEOUT, self.listener.accept())
            .await
            .context("the client didn't connect")??;
        Ok(FakeConnection {
            ws: accept_async(stream).await?,
        })
    }

    /// Whether the client connects again within `duration`.
    async fn connects_within(&self, duration: Duration) -> bool {
        tokio::time::timeout(duration, self.listener.accept())
            .await
            .is_ok()
    }
}

struct FakeConnection {
    ws: WebSocketStream<TcpStream>,
}

impl FakeConnection {
    async fn receive(&mut self) -> anyhow::Result<ClientMessage> {
        loop {
            let message = tokio::time::timeout(TIMEOUT, self.ws.next())
                .await
                .context("the client didn't send a message")?
                .context("the client disconnected")??;
            if let Message::Text(text) = message {
                return serde_json::from_str::<JsonValue>(&text)?.try_into();
            }
        }
    }

    /// Waits for the client to modify its query set, and returns the new
    /// version and the queries it added.
    async fn receive_query_set(&mut self) -> anyhow::Result<(QuerySetVersion, Vec<QueryId>)> {
        loop {
            if let ClientMessage::ModifyQuerySet {
                new_version,
                modifications,
                ..
            } = self.receive().await?
            {
                let added = modifications
                    .into_iter()
                    .filter_map(|modification| match modification {
                        QuerySetModification::Add(query) => Some(query.query_id),
                        QuerySetModification::Remove { .. } => None,
                    })
                    .collect();
                return Ok((new_version, added));
            }
        }
    }

    async fn receive_authenticate(&mut self) -> anyhow::Result<AuthenticationToken> {
        loop {
            if let ClientMessage::Authenticate { token, .. } = self.receive().await? {
                return Ok(token);
            }
        }
    }

    async fn send(&mut self, message: ServerMessage) -> anyhow::Result<()> {
        self.ws
            .send(Message::Text(JsonValue::from(message).to_string().into()))
            .await?;
        Ok(())
    }
}

/// A transition from `start` that sets `query_id` to `value`.
fn transition(
    start: StateVersion,
    query_set: QuerySetVersion,
    ts: Timestamp,
    query_id: QueryId,
    value: Value,
) -> (ServerMessage, StateVersion) {
    let end = StateVersion {
        query_set,
        identity: start.identity,
        ts,
    };
    let message = ServerMessage::Transition {
        start_version: start,
        end_version: end,
        modifications: vec![StateModification::QueryUpdated {
            query_id,
            value,
            journal: None,
            log_lines: LogLinesMessage(vec![]),
        }],
        client_clock_skew: None,
        server_ts: None,
    };
    (message, end)
}

fn auth_error() -> ServerMessage {
    ServerMessage::AuthError {
        error_message: "token expired".into(),
        base_version: None,
        auth_update_attempted: None,
    }
}

fn ts(n: u64) -> anyhow::Result<Timestamp> {
    Timestamp::try_from(n)
}

#[tokio::test]
async fn test_auth_callback_waiting_on_the_connection_doesnt_deadlock() -> anyhow::Result<()> {
    let server = FakeServer::start().await?;
    let mut client = ConvexClient::new(&server.url()?).await?;
    // On reconnect, the callback waits until the client has connected again,
    // like a callback waiting for the network to come back.
    let reconnected = Arc::new(Notify::new());
    let callback_reconnected = reconnected.clone();
    client
        .set_auth_callback(Some(Box::new(move |force_refresh| {
            let reconnected = callback_reconnected.clone();
            Box::pin(async move {
                if force_refresh {
                    reconnected.notified().await;
                }
                Ok(AuthenticationToken::User("token".into()))
            })
        })))
        .await;
    let mut connection = server.accept().await?;
    connection.receive_authenticate().await?;

    // The token expires, and more server messages arrive while the client
    // worker backs off before reconnecting.
    connection.send(auth_error()).await?;
    for _ in 0..3 {
        connection.send(ServerMessage::Ping).await?;
    }

    // The WebSocket worker still handles the reconnect.
    let mut connection = server.accept().await?;
    reconnected.notify_one();
    assert_eq!(
        connection.receive_authenticate().await?,
        AuthenticationToken::User("token".into())
    );
    Ok(())
}

#[tokio::test]
async fn test_messages_from_the_previous_connection_are_ignored() -> anyhow::Result<()> {
    let server = FakeServer::start().await?;
    let mut client = ConvexClient::new(&server.url()?).await?;
    let mut connection = server.accept().await?;
    let mut subscription =
        tokio::time::timeout(TIMEOUT, client.subscribe("counter:get", btreemap! {})).await??;
    let (query_set, queries) = connection.receive_query_set().await?;
    let (message, version) = transition(
        StateVersion::initial(),
        query_set,
        ts(1)?,
        queries[0],
        1.into(),
    );
    connection.send(message).await?;
    assert_eq!(
        subscription.next().await,
        Some(FunctionResult::Value(1.into()))
    );

    // An auth error makes the client reconnect, but the server sends another
    // transition on the old connection first.
    connection.send(auth_error()).await?;
    let (stale, _) = transition(version, query_set, ts(2)?, queries[0], 2.into());
    connection.send(stale).await?;

    let mut connection = server.accept().await?;
    let (query_set, queries) = connection.receive_query_set().await?;
    let (message, _) = transition(
        StateVersion::initial(),
        query_set,
        ts(3)?,
        queries[0],
        3.into(),
    );
    connection.send(message).await?;
    assert_eq!(
        tokio::time::timeout(TIMEOUT, subscription.next()).await?,
        Some(FunctionResult::Value(3.into()))
    );
    // The stale transition didn't make the client reconnect again.
    assert!(!server.connects_within(Duration::from_secs(1)).await);
    Ok(())
}
