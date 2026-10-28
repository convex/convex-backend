use std::{
    collections::BTreeMap,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
    },
};

use application::{
    api::{
        is_recoverable_subscription_stream_failure,
        mark_recoverable_subscription_stream_failure,
        ApplicationApi,
        RecoverableSubscriptionStreamFailure,
        SubscriptionClient,
        SubscriptionTrait,
        SubscriptionValidity,
    },
    test_helpers::ApplicationTestExt,
    Application,
};
use async_trait::async_trait;
use common::{
    http::{
        RequestDestination,
        ResolvedHostname,
        ResolvedHostnameSource,
    },
    runtime::Runtime,
    types::Timestamp,
    version::ClientVersion,
    RequestId,
    RequestMetadata,
};
use database::Token;
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    future::{
        self,
        BoxFuture,
    },
    FutureExt,
};
use keybroker::Identity;
use must_let::must_let;
use runtime::testing::TestRuntime;
use serde_json::json;
use sync_types::{
    types::{
        ClientEvent,
        SerializedArgs,
    },
    AuthenticationToken,
    ClientMessage,
    IdentityVersion,
    Query,
    QueryId,
};
use tokio::sync::{
    mpsc,
    oneshot,
};

use super::{
    authentication_update_error,
    join_update_query_tasks,
    measurable_unbounded_channel,
    SingleFlightReceiver,
    SyncWorker,
    UpdateQueryOutcome,
    HEARTBEAT_INTERVAL,
    UPDATE_QUERY_CONCURRENCY,
};
use crate::{
    state::QueryToFetch,
    subscription_reconnect::SubscriptionReconnectRateLimiter,
    ServerMessage,
    SyncWorkerConfig,
};

struct FailingSubscription;

#[async_trait]
impl SubscriptionTrait for FailingSubscription {
    fn wait_for_invalidation(&self) -> BoxFuture<'static, anyhow::Result<Option<Timestamp>>> {
        futures::future::pending().boxed()
    }

    async fn extend_validity(&self, _new_ts: Timestamp) -> anyhow::Result<SubscriptionValidity> {
        Err(mark_recoverable_subscription_stream_failure(
            anyhow::anyhow!("extend validity failed"),
        ))
    }
}

struct FailingSubscriptionClient;

#[async_trait]
impl SubscriptionClient for FailingSubscriptionClient {
    async fn subscribe(&self, _token: Token) -> anyhow::Result<Arc<dyn SubscriptionTrait>> {
        Err(mark_recoverable_subscription_stream_failure(
            anyhow::anyhow!("subscribe failed"),
        ))
    }
}

fn query() -> anyhow::Result<Query> {
    Ok(Query {
        query_id: QueryId::new(0),
        udf_path: "sync:accountBalance".parse()?,
        args: SerializedArgs::from_args(vec![])?,
        journal: None,
        component_path: None,
    })
}

fn host() -> ResolvedHostname {
    ResolvedHostname {
        deployment_name: String::new(),
        destination: RequestDestination::ConvexCloud,
        source: ResolvedHostnameSource::Local,
    }
}

fn assert_recoverable_after_task_boundary(error: anyhow::Error) {
    assert!(is_recoverable_subscription_stream_failure(&error));
    assert!(error
        .downcast_ref::<RecoverableSubscriptionStreamFailure>()
        .is_some());
}

async fn delayed_worker(
    rt: &TestRuntime,
) -> anyhow::Result<(
    SyncWorker<TestRuntime>,
    mpsc::UnboundedSender<(ClientMessage, tokio::time::Instant)>,
    SingleFlightReceiver,
)> {
    let application = Application::new_for_tests(rt).await?;
    application.load_udf_tests_modules().await?;
    let (client_tx, client_rx) = mpsc::unbounded_channel();
    let (server_tx, server_rx) = measurable_unbounded_channel();
    let limiter = Arc::new(SubscriptionReconnectRateLimiter::new(0.01)?);
    let _first = limiter.reserve(0, 1, rt.monotonic_now());
    let mut worker = SyncWorker::new(
        Arc::new(application),
        rt.clone(),
        host(),
        SyncWorkerConfig {
            subscription_reconnect_rate_limiter: Some(limiter),
            ..Default::default()
        },
        client_rx,
        server_tx,
        Box::new(|_| {}),
        0,
        RequestMetadata::new_for_test_with_http_metadata(),
    );
    worker.insert_test_query(query()?)?;
    Ok((worker, client_tx, server_rx))
}

#[convex_macro::test_runtime]
async fn run_update_queries_preserves_extend_validity_recoverability(
    rt: TestRuntime,
) -> anyhow::Result<()> {
    let application = Arc::new(Application::new_for_tests(&rt).await?);
    let api: Arc<dyn ApplicationApi> = application;
    let host = host();
    let new_ts = *api.latest_timestamp(&host, RequestId::new()).await?;
    let query = query()?;
    let mut subscriptions = BTreeMap::new();
    subscriptions.insert(
        query.query_id,
        Arc::new(FailingSubscription) as Arc<dyn SubscriptionTrait>,
    );

    let result = SyncWorker::run_update_queries(
        api,
        rt,
        host,
        RequestMetadata::new_for_test_with_http_metadata(),
        vec![QueryToFetch {
            query,
            has_run_before: true,
        }],
        Identity::system(),
        false,
        ClientVersion::unknown(),
        0,
        Arc::new(FailingSubscriptionClient),
        subscriptions,
        new_ts,
    )
    .await;
    let Err(error) = result else {
        anyhow::bail!("extend validity failure should escape update task");
    };
    assert_recoverable_after_task_boundary(error);
    Ok(())
}

#[convex_macro::test_runtime]
async fn run_update_queries_preserves_subscribe_recoverability(
    rt: TestRuntime,
) -> anyhow::Result<()> {
    let application = Application::new_for_tests(&rt).await?;
    application.load_udf_tests_modules().await?;
    let api: Arc<dyn ApplicationApi> = Arc::new(application);
    let host = host();
    let new_ts = *api.latest_timestamp(&host, RequestId::new()).await?;

    let result = SyncWorker::run_update_queries(
        api,
        rt,
        host,
        RequestMetadata::new_for_test_with_http_metadata(),
        vec![QueryToFetch {
            query: query()?,
            has_run_before: false,
        }],
        Identity::system(),
        false,
        ClientVersion::unknown(),
        0,
        Arc::new(FailingSubscriptionClient),
        BTreeMap::new(),
        new_ts,
    )
    .await;
    let Err(error) = result else {
        anyhow::bail!("subscribe failure should escape update task");
    };
    assert_recoverable_after_task_boundary(error);
    Ok(())
}

#[convex_macro::test_runtime]
async fn recoverable_update_failure_stops_query_fanout(_rt: TestRuntime) -> anyhow::Result<()> {
    let started = Arc::new(AtomicUsize::new(0));
    let pending_count = UPDATE_QUERY_CONCURRENCY + 1;
    let mut tasks: Vec<BoxFuture<'static, anyhow::Result<UpdateQueryOutcome>>> = vec![async {
        Ok(UpdateQueryOutcome::RecoverableFailure(
            mark_recoverable_subscription_stream_failure(anyhow::anyhow!("stream closed")),
        ))
    }
    .boxed()];
    for _ in 0..pending_count {
        let started = started.clone();
        tasks.push(
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                future::pending().await
            }
            .boxed(),
        );
    }

    let mut joined = Box::pin(join_update_query_tasks(
        "test_update_query",
        tasks.into_iter(),
    ));
    let mut result = None;
    for _ in 0..100 {
        if let Some(completed) = joined.as_mut().now_or_never() {
            result = Some(completed);
            break;
        }
        tokio::task::yield_now().await;
    }
    let Some(result) = result else {
        anyhow::bail!("recoverable failure did not stop query fanout");
    };
    let Err(error) = result else {
        anyhow::bail!("recoverable failure should stop query fanout");
    };
    assert_recoverable_after_task_boundary(error);
    assert!(started.load(Ordering::SeqCst) < pending_count);
    Ok(())
}

#[convex_macro::test_runtime]
async fn delayed_close_pings_then_client_work_bypasses(rt: TestRuntime) -> anyhow::Result<()> {
    let (mut worker, client_tx, mut server_rx) = delayed_worker(&rt).await?;
    let (result_tx, result_rx) = oneshot::channel();
    let _worker_handle = rt.spawn("delayed_close", async move {
        worker.delay_recoverable_subscription_failure().await;
        _ = result_tx.send(());
    });
    tokio::task::yield_now().await;
    let ping_deadline = rt.monotonic_now() + HEARTBEAT_INTERVAL;
    let partial_heartbeat = HEARTBEAT_INTERVAL / 2;
    rt.advance_time(partial_heartbeat).await;
    client_tx.send((
        ClientMessage::Event(ClientEvent {
            event_type: "ClientConnect".to_owned(),
            event: json!([]),
        }),
        rt.monotonic_now(),
    ))?;
    tokio::task::yield_now().await;
    rt.advance_time(HEARTBEAT_INTERVAL - partial_heartbeat)
        .await;
    must_let!(let Some((ServerMessage::Ping {}, sent_at)) = server_rx.next().await);
    assert_eq!(sent_at, ping_deadline);
    client_tx.send((
        ClientMessage::Authenticate {
            base_version: IdentityVersion::default(),
            token: AuthenticationToken::None,
        },
        rt.monotonic_now(),
    ))?;
    result_rx.await?;
    Ok(())
}
