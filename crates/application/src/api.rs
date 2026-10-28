use std::{
    ops::Bound,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use common::{
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentId,
        ExportPath,
        PublicFunctionPath,
    },
    http::ResolvedHostname,
    runtime::Runtime,
    types::{
        AllowedVisibility,
        ConvexOrigin,
        FunctionCaller,
        QueryInvocation,
        RepeatableTimestamp,
    },
    RequestContext,
    RequestId,
};
use database::{
    Database,
    LogReader,
    ReadSet,
    Subscription,
    Token,
};
use file_storage::FileStream;
use futures::{
    future::BoxFuture,
    stream::BoxStream,
    FutureExt,
};
use headers::{
    ContentLength,
    ContentType,
};
use keybroker::Identity;
use model::{
    file_storage::FileStorageId,
    session_requests::types::SessionRequestIdentifier,
};
use sync_types::{
    types::SerializedArgs,
    AuthenticationToken,
    SerializedQueryJournal,
    Timestamp,
};
use udf::{
    HttpActionRequest,
    HttpActionResponseStreamer,
};
use value::{
    sha256::Sha256Digest,
    DeveloperDocumentId,
};

use crate::{
    Application,
    FunctionError,
    FunctionReturn,
    RedactedActionError,
    RedactedActionReturn,
    RedactedMutationError,
    RedactedMutationReturn,
    RedactedQueryReturn,
};

pub enum ExecuteQueryTimestamp {
    // Execute the query at the latest timestamp.
    Latest,
    // Execute the query at a given timestamp.
    At(Timestamp),
}

// A trait that abstracts the backend API. It all state and validation logic
// so http routes can be kept thin and stateless. The implementor is also
// responsible for routing the request to the appropriate backend in the hosted
// version of Convex.
#[async_trait]
pub trait ApplicationApi: Send + Sync {
    async fn authenticate(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        auth_token: AuthenticationToken,
    ) -> anyhow::Result<Identity>;

    /// Execute a public query on the root app. This method is used by the sync
    /// worker and HTTP API for the majority of traffic as the main entry point
    /// for queries.
    async fn execute_public_query(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        ts: ExecuteQueryTimestamp,
        journal: Option<SerializedQueryJournal>,
        invocation: Option<QueryInvocation>,
    ) -> anyhow::Result<RedactedQueryReturn>;

    /// Execute an admin query for a particular component. This method is used
    /// by the sync worker for running queries for the dashboard and only works
    /// for admin or system identity.
    async fn execute_admin_query(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        ts: ExecuteQueryTimestamp,
        journal: Option<SerializedQueryJournal>,
        invocation: Option<QueryInvocation>,
    ) -> anyhow::Result<RedactedQueryReturn>;

    /// Execute a public mutation on the root app.
    async fn execute_public_mutation(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        // Identifier used to make this mutation idempotent.
        mutation_identifier: Option<SessionRequestIdentifier>,
        // The length of the mutation queue at the time the mutation was executed.
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<RedactedMutationReturn, RedactedMutationError>>;

    /// Execute an admin mutation for a particular component for the dashboard.
    async fn execute_admin_mutation(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        mutation_identifier: Option<SessionRequestIdentifier>,
        // The length of the mutation queue at the time the mutation was executed.
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<RedactedMutationReturn, RedactedMutationError>>;

    /// Execute a public action on the root app.
    async fn execute_public_action(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<RedactedActionReturn, RedactedActionError>>;

    /// Execute an admin action for a particular component for the dashboard.
    async fn execute_admin_action(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<RedactedActionReturn, RedactedActionError>>;

    /// Execute an HTTP action on the root app.
    async fn execute_http_action(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        http_request_metadata: HttpActionRequest,
        identity: Identity,
        caller: FunctionCaller,
        response_streamer: HttpActionResponseStreamer,
    ) -> anyhow::Result<()>;

    /// For the dashboard (and the CLI), run any function in any component
    /// without knowing its type. This function requires admin identity for
    /// calling functions outside the root component.
    async fn execute_any_function(
        &self,
        host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<FunctionReturn, FunctionError>>;

    async fn latest_timestamp(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
    ) -> anyhow::Result<RepeatableTimestamp>;

    async fn check_store_file_authorization(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
        token: &str,
        validity: Duration,
    ) -> anyhow::Result<ComponentId>;

    async fn store_file(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
        origin: ConvexOrigin,
        component: ComponentId,
        content_length: Option<ContentLength>,
        content_type: Option<ContentType>,
        expected_sha256: Option<Sha256Digest>,
        body: BoxStream<'_, anyhow::Result<Bytes>>,
    ) -> anyhow::Result<DeveloperDocumentId>;

    async fn get_file_range(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
        origin: ConvexOrigin,
        component: ComponentId,
        file_storage_id: FileStorageId,
        range: (Bound<u64>, Bound<u64>),
    ) -> anyhow::Result<FileStream>;

    async fn get_file(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
        origin: ConvexOrigin,
        component: ComponentId,
        file_storage_id: FileStorageId,
    ) -> anyhow::Result<FileStream>;

    // Returns a fallible subscription client. The implementation is not required to
    // recover from transient errors with the underlying connection or stream. The
    // client is responsible to Drop the client and create a new one on any system
    // errors. The intended use is to create a single client for every new web
    // socket connection. NOTE: We might eventually strengthen the requirement for
    // the implementation and require it to reconnect internally but easier to
    // start this way.
    async fn subscription_client(
        &self,
        host: &ResolvedHostname,
    ) -> anyhow::Result<Box<dyn SubscriptionClient>>;

    /// To be used for metrics only.
    async fn partition_id(&self, host: &ResolvedHostname) -> anyhow::Result<u64>;
}

// Implements ApplicationApi via Application.
#[async_trait]
impl<RT: Runtime> ApplicationApi for Application<RT> {
    async fn authenticate(
        &self,
        _host: &ResolvedHostname,
        _request_context: RequestContext,
        auth_token: AuthenticationToken,
    ) -> anyhow::Result<Identity> {
        let validate_time = self.runtime().system_time();
        self.authenticate(auth_token, validate_time).await
    }

    async fn execute_public_query(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        ts: ExecuteQueryTimestamp,
        journal: Option<SerializedQueryJournal>,
        invocation: Option<QueryInvocation>,
    ) -> anyhow::Result<RedactedQueryReturn> {
        anyhow::ensure!(
            caller.allowed_visibility() == AllowedVisibility::PublicOnly,
            "This method should not be used by internal callers."
        );
        let ts = match ts {
            ExecuteQueryTimestamp::Latest => *self.now_ts_for_reads(),
            ExecuteQueryTimestamp::At(ts) => ts,
        };
        self.read_only_udf_at_ts(
            request_context,
            PublicFunctionPath::RootExport(path),
            args,
            identity,
            ts,
            journal,
            caller,
            invocation.unwrap_or(QueryInvocation::Fresh),
        )
        .await
    }

    async fn execute_admin_query(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        ts: ExecuteQueryTimestamp,
        journal: Option<SerializedQueryJournal>,
        invocation: Option<QueryInvocation>,
    ) -> anyhow::Result<RedactedQueryReturn> {
        anyhow::ensure!(
            path.component.is_root() || identity.is_admin() || identity.is_system(),
            "Only admin or system users can call functions on non-root components directly"
        );
        let ts = match ts {
            ExecuteQueryTimestamp::Latest => *self.now_ts_for_reads(),
            ExecuteQueryTimestamp::At(ts) => ts,
        };
        self.read_only_udf_at_ts(
            request_context,
            PublicFunctionPath::Component(path),
            args,
            identity,
            ts,
            journal,
            caller,
            invocation.unwrap_or(QueryInvocation::Fresh),
        )
        .await
    }

    async fn execute_public_mutation(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        // Identifier used to make this mutation idempotent.
        mutation_identifier: Option<SessionRequestIdentifier>,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<RedactedMutationReturn, RedactedMutationError>> {
        anyhow::ensure!(
            caller.allowed_visibility() == AllowedVisibility::PublicOnly,
            "This method should not be used by internal callers."
        );
        self.mutation_udf(
            request_context,
            PublicFunctionPath::RootExport(path),
            args,
            identity,
            mutation_identifier,
            caller,
            mutation_queue_length,
        )
        .await
    }

    async fn execute_admin_mutation(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
        mutation_identifier: Option<SessionRequestIdentifier>,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<RedactedMutationReturn, RedactedMutationError>> {
        anyhow::ensure!(
            path.component.is_root() || identity.is_admin() || identity.is_system(),
            "Only admin or system users can call functions on non-root components directly"
        );
        self.mutation_udf(
            request_context,
            PublicFunctionPath::Component(path),
            args,
            identity,
            mutation_identifier,
            caller,
            mutation_queue_length,
        )
        .await
    }

    async fn execute_public_action(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: ExportPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<RedactedActionReturn, RedactedActionError>> {
        anyhow::ensure!(
            caller.allowed_visibility() == AllowedVisibility::PublicOnly,
            "This method should not be used by internal callers."
        );
        self.action_udf(
            request_context,
            PublicFunctionPath::RootExport(path),
            args,
            identity,
            caller,
        )
        .await
    }

    async fn execute_admin_action(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<RedactedActionReturn, RedactedActionError>> {
        anyhow::ensure!(
            path.component.is_root() || identity.is_admin() || identity.is_system(),
            "Only admin or system users can call functions on non-root components directly"
        );
        self.action_udf(
            request_context,
            PublicFunctionPath::Component(path),
            args,
            identity,
            caller,
        )
        .await
    }

    async fn execute_any_function(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<FunctionReturn, FunctionError>> {
        anyhow::ensure!(
            path.component.is_root() || identity.is_admin() || identity.is_system(),
            "Only admin or system users can call functions on non-root components directly"
        );
        self.any_udf(request_context, path, args, identity, caller)
            .await
    }

    async fn latest_timestamp(
        &self,
        _host: &ResolvedHostname,
        _request_id: RequestId,
    ) -> anyhow::Result<RepeatableTimestamp> {
        Ok(self.now_ts_for_reads())
    }

    async fn execute_http_action(
        &self,
        _host: &ResolvedHostname,
        request_context: RequestContext,
        http_request_metadata: HttpActionRequest,
        identity: Identity,
        caller: FunctionCaller,
        response_streamer: HttpActionResponseStreamer,
    ) -> anyhow::Result<()> {
        self.http_action_udf(
            request_context,
            http_request_metadata,
            identity,
            caller,
            response_streamer,
        )
        .await
    }

    async fn check_store_file_authorization(
        &self,
        _host: &ResolvedHostname,
        _request_id: RequestId,
        token: &str,
        validity: Duration,
    ) -> anyhow::Result<ComponentId> {
        self.key_broker()
            .check_store_file_authorization(&self.runtime, token, validity)
    }

    async fn store_file(
        &self,
        _host: &ResolvedHostname,
        _request_id: RequestId,
        _origin: ConvexOrigin,
        component: ComponentId,
        content_length: Option<ContentLength>,
        content_type: Option<ContentType>,
        expected_sha256: Option<Sha256Digest>,
        body: BoxStream<'_, anyhow::Result<Bytes>>,
    ) -> anyhow::Result<DeveloperDocumentId> {
        self.store_file(
            component,
            content_length,
            content_type,
            expected_sha256,
            body,
        )
        .await
    }

    async fn get_file_range(
        &self,
        _host: &ResolvedHostname,
        _request_id: RequestId,
        _origin: ConvexOrigin,
        component: ComponentId,
        file_storage_id: FileStorageId,
        range: (Bound<u64>, Bound<u64>),
    ) -> anyhow::Result<FileStream> {
        self.get_file_range(component, file_storage_id, range).await
    }

    async fn get_file(
        &self,
        _host: &ResolvedHostname,
        _request_id: RequestId,
        _origin: ConvexOrigin,
        component: ComponentId,
        file_storage_id: FileStorageId,
    ) -> anyhow::Result<FileStream> {
        self.get_file(component, file_storage_id).await
    }

    async fn subscription_client(
        &self,
        _host: &ResolvedHostname,
    ) -> anyhow::Result<Box<dyn SubscriptionClient>> {
        Ok(Box::new(ApplicationSubscriptionClient {
            database: self.database.clone(),
        }))
    }

    async fn partition_id(&self, _host: &ResolvedHostname) -> anyhow::Result<u64> {
        // Not relevant; just return something
        Ok(0)
    }
}

#[async_trait]
pub trait SubscriptionClient: Send + Sync {
    async fn subscribe(&self, token: Token) -> anyhow::Result<Arc<dyn SubscriptionTrait>>;
}

/// A remote subscription stream stopped for a condition that a new sync session
/// can retry.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("subscription stream became unavailable")]
pub struct RecoverableSubscriptionStreamFailure;

pub fn mark_recoverable_subscription_stream_failure(mut error: anyhow::Error) -> anyhow::Error {
    if error
        .downcast_ref::<RecoverableSubscriptionStreamFailure>()
        .is_none()
    {
        error = error.context(RecoverableSubscriptionStreamFailure);
    }
    error
}

pub fn is_recoverable_subscription_stream_failure(error: &anyhow::Error) -> bool {
    error.is::<RecoverableSubscriptionStreamFailure>()
}

struct ApplicationSubscriptionClient<RT: Runtime> {
    database: Database<RT>,
}

#[async_trait]
impl<RT: Runtime> SubscriptionClient for ApplicationSubscriptionClient<RT> {
    async fn subscribe(&self, token: Token) -> anyhow::Result<Arc<dyn SubscriptionTrait>> {
        let inner = self.database.subscribe(token.clone()).await?;
        Ok(Arc::new(ApplicationSubscription {
            initial_ts: token.ts(),
            reads: token.reads_owned(),
            inner,
            log: self.database.log().clone(),
        }))
    }
}

pub enum SubscriptionValidity {
    /// The subscription is valid.
    Valid,
    /// The subscription may no longer be valid.
    /// This result can be returned spuriously even if there were no conflicting
    /// writes.
    Invalid {
        /// The earliest conflicting timestamp, if known. This is not guaranteed
        /// to be known.
        invalid_ts: Option<Timestamp>,
    },
}

#[async_trait]
pub trait SubscriptionTrait: Send + Sync {
    /// Returns a future that completes after the subscription becomes invalid.
    /// The future yields the invalidation timestamp, if known; this is the same
    /// thing as [`SubscriptionValidity`]'s `invalid_ts`.
    fn wait_for_invalidation(&self) -> BoxFuture<'static, anyhow::Result<Option<Timestamp>>>;

    /// Checks if the subscription is still valid as of `new_ts`. See comments
    /// on [`SubscriptionValidity`].
    async fn extend_validity(&self, new_ts: Timestamp) -> anyhow::Result<SubscriptionValidity>;
}

struct ApplicationSubscription {
    inner: Subscription,
    log: LogReader,

    reads: Arc<ReadSet>,
    // The initial timestamp the subscription was created at. This is known
    // to be valid.
    initial_ts: Timestamp,
}

#[async_trait]
impl SubscriptionTrait for ApplicationSubscription {
    fn wait_for_invalidation(&self) -> BoxFuture<'static, anyhow::Result<Option<Timestamp>>> {
        self.inner.wait_for_invalidation().map(Ok).boxed()
    }

    #[fastrace::trace]
    async fn extend_validity(&self, new_ts: Timestamp) -> anyhow::Result<SubscriptionValidity> {
        if new_ts < self.initial_ts {
            // new_ts is before the initial subscription timestamp.
            return Ok(SubscriptionValidity::Invalid { invalid_ts: None });
        }

        // The inner subscription is periodically updated by the subscription
        // worker.
        let Some(current_ts) = self.inner.current_ts() else {
            // Subscription is no longer valid. We could check validity from end_ts
            // to new_ts, but this is likely to fail and is potentially unbounded amount of
            // work, so we return false here. This is valid per the function contract.
            return Ok(SubscriptionValidity::Invalid {
                invalid_ts: self.inner.invalid_ts(),
            });
        };

        let current_token = Token::new(self.reads.clone(), current_ts);
        Ok(match self.log.refresh_token(current_token, new_ts)? {
            Ok(_new_token) => SubscriptionValidity::Valid,
            Err(invalid_ts) => {
                // Subscription validity can't be extended. Note that returning false
                // here also doesn't mean there is a conflict.
                SubscriptionValidity::Invalid { invalid_ts }
            },
        })
    }
}
