use std::{
    env,
    io::ErrorKind,
    marker::PhantomData,
    mem,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
        OnceLock,
        Weak,
    },
    time::Duration,
};

use ::metrics::StaticMetricLabel;
use anyhow::Context as _;
use common::{
    errors::{
        database_deadlock_error,
        database_operational_error,
        database_timeout_error,
        DatabaseOperationalError,
        DatabaseTimeoutError,
    },
    fastrace_helpers::FutureExt as _,
    knobs::{
        DATABASE_USE_PREPARED_STATEMENTS,
        MYSQL_INACTIVE_CONNECTION_LIFETIME,
        MYSQL_MAX_CONNECTIONS,
        MYSQL_MAX_CONNECTION_LIFETIME,
        MYSQL_MAX_QUERY_RETRIES,
        MYSQL_TIMEOUT,
    },
    pool_stats::{
        ConnectionPoolStats,
        ConnectionTracker,
    },
    runtime::{
        assert_send,
        tokio_spawn,
        Runtime,
    },
};
use dynfmt::{
    ArgumentSpec,
    Error,
    Format,
    FormatArgs,
    Position,
};
use fastrace::func_path;
use futures::{
    future,
    pin_mut,
    select_biased,
    Future,
    FutureExt as _,
    Stream,
    TryFutureExt as _,
    TryStreamExt,
};
use metrics::{
    ProgressCounter,
    Timer,
};
use mysql_async::{
    prelude::Queryable,
    Conn,
    DriverError,
    IoError,
    IsolationLevel,
    Opts,
    OptsBuilder,
    Params,
    Row,
    TxOpts,
    Value as MySqlValue,
};
use parking_lot::Mutex;
use prometheus::VMHistogramVec;
use rand::Rng as _;
use scopeguard::ScopeGuard;
use tokio::{
    sync::{
        Notify,
        Semaphore,
        SemaphorePermit,
    },
    task::JoinHandle,
    time::{
        sleep,
        Instant,
    },
};
use url::Url;

use crate::metrics::{
    begin_transaction_timer,
    connection_lifetime_timer,
    get_connection_timer,
    log_connection_create_failed,
    log_connection_returned_to_pool,
    log_discarded_connection,
    log_execute,
    log_large_statement,
    log_query,
    log_query_result,
    log_transaction,
    new_connection_pool_stats,
    query_progress_counter,
    DiscardReason,
    LARGE_STATEMENT_THRESHOLD,
};

fn classify_mysql_error(e: mysql_async::Error) -> anyhow::Error {
    match e {
        mysql_async::Error::Driver(
            DriverError::PoolDisconnected | DriverError::ConnectionClosed,
        )
        | mysql_async::Error::Server(mysql_async::ServerError {
            // Expected operational MySQL/Aurora errors:
            code:
            | 1290 // EROptionPreventsStatement "The MySQL server is running with the --read-only option so it cannot execute this statement"
            | 2013 // CRServerLost
            | 1053 // ERServerShutdown
            | 1040 // ERConCount "Too many connections"
            , ..
        }) => {
            database_operational_error(e.into())
        },
        mysql_async::Error::Io(ref io_error) if is_operational_mysql_io_error(io_error) => {
            database_operational_error(e.into())
        },
        mysql_async::Error::Server(mysql_async::ServerError {
            // ERUnknownError
            code: 1105,
            ref message,
            ..
        }) if message.contains("connection timed out") =>         {
            database_operational_error(e.into())
        },
        mysql_async::Error::Server(mysql_async::ServerError {
            code: ER_LOCK_DEADLOCK,
            ..
        }) => database_deadlock_error(e.into()),
        _ => e.into(),
    }
}

fn is_operational_mysql_io_error(error: &IoError) -> bool {
    let IoError::Io(error) = error else {
        return false;
    };
    matches!(
        error.kind(),
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
    ) ||
        // This DNS failure shares an ErrorKind with unrelated errors; match
        // its exact message to identify temporary resolution failures.
        error.to_string()
            == "failed to lookup address information: Temporary failure in name resolution"
}

const ER_LOCK_DEADLOCK: u16 = 1213;
pub(crate) const ER_NO_SUCH_TABLE: u16 = 1146;

pub(crate) fn is_no_such_table_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|error| error.downcast_ref::<mysql_async::ServerError>())
        .any(|database_error| database_error.code == ER_NO_SUCH_TABLE)
}

// Guard against connections hanging during bootstrapping -- which means
// instances can't start -- and during commit -- which means all future commits
// fail with OCC errors.
//
// To avoid these problems, wrap anything that talks to mysql in with_timeout
// which will panic, cleaning up all broken connections,
// if the future takes more than `MYSQL_TIMEOUT` to complete.
pub(crate) async fn with_timeout<R, Fut: Future<Output = Result<R, mysql_async::Error>>>(
    f: Fut,
) -> anyhow::Result<R> {
    with_anyhow_timeout(f.map_err(classify_mysql_error)).await
}

async fn with_anyhow_timeout<R>(f: impl Future<Output = anyhow::Result<R>>) -> anyhow::Result<R> {
    select_biased! {
        r = f.fuse() => r,
        _ = sleep(Duration::from_secs(*MYSQL_TIMEOUT)).fuse() => Err(
            anyhow::anyhow!(database_timeout_error("MySQL"))),
    }
}

struct MySQLFormatArguments {
    escaped_db_name: String,
    params: Vec<String>,
}

impl FormatArgs for MySQLFormatArguments {
    fn get_index(&self, index: usize) -> Result<Option<dynfmt::Argument<'_>>, ()> {
        self.params.get_index(index)
    }

    fn get_key(&self, key: &str) -> Result<Option<dynfmt::Argument<'_>>, ()> {
        match key {
            "db_name" => Ok(Some(&self.escaped_db_name)),
            _ => panic!("Unexpected named argument {key}"),
        }
    }
}

const DB_NAME_ARGUMENT_PATTERN: &str = "@db_name";

// Formats @db_name and ?
struct MySQLRawStatementFormat;

impl<'f> Format<'f> for MySQLRawStatementFormat {
    type Iter = impl Iterator<Item = Result<ArgumentSpec<'f>, Error<'f>>>;

    fn iter_args(&self, format: &'f str) -> Result<Self::Iter, Error<'f>> {
        let db_name_iter = format
            .match_indices(DB_NAME_ARGUMENT_PATTERN)
            .map(|(index, _)| {
                Ok(
                    ArgumentSpec::new(index, index + DB_NAME_ARGUMENT_PATTERN.len())
                        .with_position(Position::Key("db_name")),
                )
            });
        let args_iter = format
            .match_indices('?')
            .map(|(index, _)| Ok(ArgumentSpec::new(index, index + 1)));
        // The resulting iterator should be sorted.
        let mut args: Vec<_> = db_name_iter.chain(args_iter).collect();
        args.sort_by_key(|arg| match arg {
            Ok(arg) => arg.start(),
            Err(_) => 0,
        });
        Ok::<Self::Iter, _>(args.into_iter())
    }
}

// Formats a MySQL query with position parameters into a string, so it can be
// used with the text protocol.
fn format_mysql_text_protocol(
    db_name: &str,
    statement: &str,
    params: Vec<MySqlValue>,
    labels: &[StaticMetricLabel],
) -> anyhow::Result<String> {
    let args = MySQLFormatArguments {
        escaped_db_name: format!("`{db_name}`"),
        params: params
            .into_iter()
            .map(|p| match p {
                MySqlValue::NULL => "NULL".to_owned(),
                MySqlValue::Bytes(bytes) => format!("x'{}'", const_hex::display(bytes)),
                MySqlValue::Int(i) => format!("{i}"),
                MySqlValue::UInt(u) => format!("{u}"),
                // We don't use the following and I don't want to deal with escaping them.
                MySqlValue::Float(_) => panic!("Float MySQL argument not supported"),
                MySqlValue::Double(_) => panic!("Double MySQL argument not supported"),
                MySqlValue::Date(..) => panic!("Date MySQL argument not supported"),
                MySqlValue::Time(..) => panic!("Time MySQL argument not supported"),
            })
            .collect(),
    };
    let result = MySQLRawStatementFormat
        .format(statement, args)
        .map_err(|e| anyhow::anyhow!("failed to format statement: {e}"))?
        .to_string();
    if result.len() > LARGE_STATEMENT_THRESHOLD {
        log_large_statement(labels.to_vec());
    }
    Ok(result)
}

// Formats @db_name
struct MySQLPreparedStatementFormat;

impl<'f> Format<'f> for MySQLPreparedStatementFormat {
    type Iter = impl Iterator<Item = Result<ArgumentSpec<'f>, Error<'f>>>;

    fn iter_args(&self, format: &'f str) -> Result<Self::Iter, Error<'f>> {
        Ok::<Self::Iter, _>(
            format
                .match_indices(DB_NAME_ARGUMENT_PATTERN)
                .map(|(index, _)| {
                    Ok(
                        ArgumentSpec::new(index, index + DB_NAME_ARGUMENT_PATTERN.len())
                            .with_position(Position::Key("db_name")),
                    )
                }),
        )
    }
}

// Formats a MySQL query by only replacing the @db_name but leaves positional
// arguments alone. To be used with MySQL binary protocol.
fn format_mysql_binary_protocol(db_name: &str, statement: &str) -> anyhow::Result<String> {
    let args = MySQLFormatArguments {
        escaped_db_name: format!("`{db_name}`"),
        params: vec![], // No positional arguments.
    };
    Ok(MySQLPreparedStatementFormat
        .format(statement, args)
        .map_err(|e| anyhow::anyhow!("failed to format statement: {e}"))?
        .to_string())
}

/// A MySQL connection, owned by either the connection pool
/// ([`ConvexMySqlPool`]), or by an active connection ([`MySqlConnection`]).
struct PooledConnection {
    conn: Conn,
    /// See [`MYSQL_MAX_CONNECTION_LIFETIME`].
    expires_at: Instant,
    last_used: Instant,
    _established: EstablishedGuard,
}

/// Counts a connection in [`ConvexMySqlPool::established`] until dropped.
struct EstablishedGuard(Arc<AtomicUsize>);

impl EstablishedGuard {
    fn new(established: &Arc<AtomicUsize>) -> Self {
        established.fetch_add(1, Ordering::Relaxed);
        Self(established.clone())
    }
}

impl Drop for EstablishedGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl PooledConnection {
    fn new(conn: Conn, established: &Arc<AtomicUsize>) -> Self {
        // Jitter by up to 20% so that connections opened together are not all
        // recycled together.
        let jitter = rand::rng().random_range(Duration::ZERO..=*MYSQL_MAX_CONNECTION_LIFETIME / 5);
        let now = Instant::now();
        Self {
            conn,
            expires_at: now + *MYSQL_MAX_CONNECTION_LIFETIME + jitter,
            last_used: now,
            _established: EstablishedGuard::new(established),
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// When the idle worker closes this connection if it stays idle.
    fn idle_deadline(&self) -> Instant {
        (self.last_used + *MYSQL_INACTIVE_CONNECTION_LIFETIME).min(self.expires_at)
    }
}

struct IdleConnections {
    /// Ordered by `last_used` from oldest to newest.
    conns: Vec<PooledConnection>,
    /// When the idle worker next runs [`cleanup_connections`].
    next_cleanup: Instant,
}

/// An active MySQL connection from a [`ConvexMySqlPool`].
///
/// Returns the underlying connection to the pool when dropped, unless `dirty`
/// is set.
pub(crate) struct MySqlConnection<'a, RT: Runtime> {
    pool: &'a ConvexMySqlPool<RT>,
    _permit: SemaphorePermit<'a>,
    conn: Option<PooledConnection>,
    /// Set while a query or transaction has not completed successfully, since
    /// it may have left an unread result or an open transaction on the
    /// connection.
    dirty: bool,
    labels: Vec<StaticMetricLabel>,
    db_name: &'a str,
    _tracker: ConnectionTracker,
    _timer: Timer<VMHistogramVec>,
}

impl<RT: Runtime> MySqlConnection<'_, RT> {
    /// Returns the connection in `slot`, first replacing it with a new
    /// connection if it was discarded or broken.
    async fn ensure_connected<'c>(
        slot: &'c mut Option<PooledConnection>,
        pool: &ConvexMySqlPool<RT>,
    ) -> anyhow::Result<&'c mut Conn> {
        let conn = match slot.take() {
            Some(conn) if !conn.conn.is_disconnected() => conn,
            Some(_) => {
                log_discarded_connection(&pool.cluster_name, DiscardReason::Error);
                pool.create_connection().await?
            },
            None => pool.create_connection().await?,
        };
        Ok(&mut slot.insert(conn).conn)
    }

    async fn handle_errors_with_retries<R>(
        &mut self,
        mut f: impl AsyncFnMut(&mut Conn) -> anyhow::Result<R>,
        max_retries: u32,
    ) -> anyhow::Result<R> {
        let mut attempt = 0;
        loop {
            self.dirty = true;
            let (e, should_retry) =
                match f(Self::ensure_connected(&mut self.conn, self.pool).await?).await {
                    Ok(r) => {
                        self.dirty = false;
                        return Ok(r);
                    },
                    Err(e) if e.is::<DatabaseOperationalError>() => (e, attempt < max_retries),
                    Err(e) if e.is::<DatabaseTimeoutError>() => {
                        // Don't retry here as we want the caller to receive some
                        // backpressure.
                        // The mysql protocol doesn't support cancellation, so if a
                        // query times out on the client, the connection can't be
                        // reused until the server responds to the query.
                        // So don't return the connection to the pool.
                        (e, false)
                    },
                    Err(e) => return Err(e),
                };
            if should_retry {
                tracing::warn!("Retrying after MySQL error: {e:#}")
            } else {
                tracing::warn!("Discarding connection after MySQL error: {e:#}")
            }
            // Dropping the connection disconnects it in the background, since
            // that could take a long time as well. Retries use a new
            // connection in case other pooled connections are also stale.
            self.conn = None;
            log_discarded_connection(&self.pool.cluster_name, DiscardReason::Error);
            if !should_retry {
                return Err(e);
            }
            attempt += 1;
        }
    }

    async fn handle_errors<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut Conn) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        let mut f = Some(f);
        self.handle_errors_with_retries(
            async move |conn| f.take().expect("should never retry")(conn).await,
            0, /* max_retries */
        )
        .await
    }

    /// Executes multiple statements, separated by semicolons.
    #[fastrace::trace]
    pub async fn execute_many(&mut self, query: &str) -> anyhow::Result<()> {
        log_execute(self.labels.clone());
        let statement = format_mysql_text_protocol(self.db_name, query, vec![], &self.labels)?;
        self.handle_errors(async move |conn| {
            // Every statement after the first runs only as its result set is
            // consumed, and reports its error only there. Dropping the result
            // instead leaves them pending on the connection, so they run if it
            // happens to be used again and are silently lost if it is not.
            let result = with_timeout(conn.query_iter(statement)).await?;
            with_timeout(result.drop_result()).await?;
            Ok(())
        })
        .await?;
        Ok(())
    }

    /// Run a readonly query that returns one or zero results.
    #[fastrace::trace]
    pub async fn query_optional(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<Option<Row>> {
        self.query_optional_with_retries(statement, params, *MYSQL_MAX_QUERY_RETRIES)
            .await
    }

    /// Use when the result must come from the current database session.
    #[fastrace::trace]
    pub async fn query_optional_without_retries(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<Option<Row>> {
        // The retry helper reconnects closed sessions before its first attempt.
        if !self
            .conn
            .as_ref()
            .is_some_and(|conn| !conn.conn.is_disconnected())
        {
            return Err(classify_mysql_error(DriverError::ConnectionClosed.into()));
        }
        self.query_optional_with_retries(statement, params, 0).await
    }

    async fn query_optional_with_retries(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
        max_retries: u32,
    ) -> anyhow::Result<Option<Row>> {
        log_query(self.labels.clone());
        let row = if self.pool.use_prepared_statements {
            let statement = format_mysql_binary_protocol(self.db_name, statement)?;
            self.handle_errors_with_retries(
                async move |conn| with_timeout(conn.exec_first(&statement, params.clone())).await,
                max_retries,
            )
            .await?
        } else {
            let statement =
                format_mysql_text_protocol(self.db_name, statement, params, &self.labels)?;
            self.handle_errors_with_retries(
                async move |conn| with_timeout(conn.query_first(&statement)).await,
                max_retries,
            )
            .await?
        };
        if let Some(row) = &row {
            log_query_result(self.labels.clone()).add_row(row);
        }
        Ok(row)
    }

    /// Run a readonly query and collect the results, mapping them with `f`
    #[fastrace::trace]
    pub async fn query_collect<R: Send>(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
        size_hint: usize,
        f: impl Fn(Row) -> anyhow::Result<R> + Send + Sync + 'static,
    ) -> anyhow::Result<Vec<R>> {
        let labels = self.labels.clone();
        log_query(labels.clone());
        if self.pool.use_prepared_statements {
            let statement = format_mysql_binary_protocol(self.db_name, statement)?;
            assert_send(self.handle_errors_with_retries(
                async move |conn| {
                    // Any error or dropped stream after this point leaves the connection
                    // open with MySQL sending data into it, which is consumed & dropped
                    // before the connection's next query or disconnect. That can make it
                    // hard to attribute latency. Therefore we start a progress counter
                    // that will log if the stream is dropped before being consumed.
                    let progress_counter = query_progress_counter(size_hint, labels.clone());
                    Self::collect_query_stream(
                        with_timeout(
                            conn.exec_stream(&statement, Params::Positional(params.clone())),
                        )
                        .await?,
                        progress_counter,
                        labels.clone(),
                        &f,
                    )
                    .await
                },
                *MYSQL_MAX_QUERY_RETRIES,
            ))
            .await
        } else {
            let statement =
                format_mysql_text_protocol(self.db_name, statement, params, &self.labels)?;
            assert_send(self.handle_errors_with_retries(
                async move |conn| {
                    let progress_counter = query_progress_counter(size_hint, labels.clone());
                    Self::collect_query_stream(
                        with_timeout(conn.query_stream(&statement)).await?,
                        progress_counter,
                        labels.clone(),
                        &f,
                    )
                    .await
                },
                *MYSQL_MAX_QUERY_RETRIES,
            ))
            .await
        }
    }

    async fn collect_query_stream<R>(
        stream: impl Stream<Item = mysql_async::Result<Row>>,
        mut progress_counter: ProgressCounter,
        labels: Vec<StaticMetricLabel>,
        f: impl Fn(Row) -> anyhow::Result<R>,
    ) -> anyhow::Result<Vec<R>> {
        let mut result = vec![];
        pin_mut!(stream);
        let mut stats = log_query_result(labels);
        while let Some(row) = with_timeout(stream.try_next()).await? {
            progress_counter.add_processed(1);
            stats.add_row(&row);
            // `f` may be computationally intensive, and
            // `stream.try_next().await` might not yield to tokio if the rows
            // are all available at once. Avoid long poll times by intentionally
            // yielding.
            tokio::task::consume_budget().await;
            result.push(f(row)?);
        }
        progress_counter.complete();
        Ok(result)
    }

    /// Execute a SQL statement, returning the number of rows affected.
    #[fastrace::trace]
    pub async fn exec_iter(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<u64> {
        log_execute(self.labels.clone());
        let affected_rows = if self.pool.use_prepared_statements {
            let statement = format_mysql_binary_protocol(self.db_name, statement)?;
            self.handle_errors(async move |conn| {
                Ok(
                    with_timeout(conn.exec_iter(statement, Params::Positional(params)))
                        .await?
                        .affected_rows(),
                )
            })
            .await?
        } else {
            let statement =
                format_mysql_text_protocol(self.db_name, statement, params, &self.labels)?;
            self.handle_errors(async move |conn| {
                Ok(with_timeout(conn.query_iter(statement))
                    .await?
                    .affected_rows())
            })
            .await?
        };
        Ok(affected_rows)
    }

    #[fastrace::trace]
    pub async fn transaction(
        &mut self,
        db_cluster_name: &str,
        isolation: Option<IsolationLevel>,
    ) -> anyhow::Result<MySqlTransaction<'_>> {
        let timer = begin_transaction_timer(db_cluster_name);
        log_transaction(self.labels.clone());
        let mut tx_opts = TxOpts::new();
        if let Some(isolation) = isolation {
            tx_opts.with_isolation_level(isolation);
        }
        self.dirty = true;
        let conn = Self::ensure_connected(&mut self.conn, self.pool).await?;
        let inner = with_timeout(conn.start_transaction(tx_opts)).await?;
        timer.finish();
        Ok(MySqlTransaction {
            inner,
            use_prepared_statements: self.pool.use_prepared_statements,
            db_name: self.db_name,
            labels: &self.labels,
            dirty: &mut self.dirty,
        })
    }
}

impl<RT: Runtime> Drop for MySqlConnection<'_, RT> {
    fn drop(&mut self) {
        let Some(conn) = self.conn.take() else {
            return;
        };
        if self.dirty {
            // Don't return dirty connections to the pool. Such a connection would only be
            // cleaned up the next time that it's checked out from the pool and used.
            // Instead, drop it here; it'll be closed in the background, releasing any held
            // locks promptly.
            log_discarded_connection(&self.pool.cluster_name, DiscardReason::Dirty);
            return;
        }
        self.pool.return_connection(conn);
    }
}

pub(crate) struct MySqlTransaction<'a> {
    inner: mysql_async::Transaction<'a>,
    use_prepared_statements: bool,
    db_name: &'a str,
    labels: &'a [StaticMetricLabel],
    /// The [`MySqlConnection`]'s `dirty` flag, cleared on commit
    dirty: &'a mut bool,
}

impl MySqlTransaction<'_> {
    /// Executes the given statement and returns the first row of the first
    /// result set.
    pub async fn exec_first(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<Option<Row>> {
        let future = if self.use_prepared_statements {
            let statement = format_mysql_binary_protocol(self.db_name, statement)?;
            self.inner.exec_first(statement, Params::Positional(params))
        } else {
            let statement =
                format_mysql_text_protocol(self.db_name, statement, params, self.labels)?;
            self.inner.query_first(statement)
        };
        with_timeout(future).await
    }

    /// Executes the given statement and drops the result.
    pub async fn exec_drop(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<()> {
        if !self.use_prepared_statements {
            return self.query_drop(statement, params).await;
        }
        let statement = format_mysql_binary_protocol(self.db_name, statement)?;
        with_timeout(self.inner.exec_drop(statement, Params::Positional(params))).await
    }

    /// Execute a SQL statement, returning the number of rows affected.
    pub async fn exec_iter(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<u64> {
        if !self.use_prepared_statements {
            return self.query_iter(statement, params).await;
        }
        let statement = format_mysql_binary_protocol(self.db_name, statement)?;
        Ok(
            with_timeout(self.inner.exec_iter(statement, Params::Positional(params)))
                .await?
                .affected_rows(),
        )
    }

    /// Executes a raw query using the text protocol, even when the connection
    /// is configured to use prepared statements.
    pub async fn query_drop(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<()> {
        let statement = format_mysql_text_protocol(self.db_name, statement, params, self.labels)?;
        with_timeout(self.inner.query_drop(statement)).await
    }

    /// Like [`Self::query_drop`], returning the number of rows affected.
    pub async fn query_iter(
        &mut self,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<u64> {
        let statement = format_mysql_text_protocol(self.db_name, statement, params, self.labels)?;
        Ok(with_timeout(self.inner.query_iter(statement))
            .await?
            .affected_rows())
    }

    pub async fn commit(self) -> anyhow::Result<()> {
        with_timeout(self.inner.commit()).await?;
        *self.dirty = false;
        Ok(())
    }
}

/// How long new connections reuse an RDS IAM auth token. RDS rejects a token
/// for new logins 15 minutes after it was minted.
const RDS_IAM_TOKEN_REUSE: Duration = Duration::from_mins(5);

/// Logs new connections in to an RDS endpoint with IAM auth tokens.
struct RdsIamAuth {
    cached_opts: Mutex<Option<RdsIamCachedOpts>>,
}

struct RdsIamCachedOpts {
    opts: Opts,
    minted_at: Instant,
}

impl RdsIamAuth {
    async fn authed_opts(&self, base_opts: &Opts) -> anyhow::Result<Opts> {
        if let Some(cached) = self
            .cached_opts
            .lock()
            .as_ref()
            .filter(|cached| cached.minted_at.elapsed() < RDS_IAM_TOKEN_REUSE)
        {
            return Ok(cached.opts.clone());
        }
        let minted_at = Instant::now();
        let token = aws_utils::rds::get_rds_auth_token(
            base_opts.ip_or_hostname(),
            base_opts.tcp_port(),
            base_opts.user().context("user must be set")?,
        )
        .await?
        .to_string();
        let mut cached_opts = self.cached_opts.lock();
        // Check again to avoid minting new `Opts` objects if there is a race
        if let Some(cached) = cached_opts
            .as_ref()
            .filter(|cached| cached.minted_at.elapsed() < RDS_IAM_TOKEN_REUSE)
        {
            return Ok(cached.opts.clone());
        }
        Ok(cached_opts
            .insert(RdsIamCachedOpts {
                opts: OptsBuilder::from_opts(base_opts.clone())
                    .pass(Some(token))
                    .into(),
                minted_at,
            })
            .opts
            .clone())
    }
}

/// A MySQL connection pool.
pub struct ConvexMySqlPool<RT: Runtime> {
    pub(crate) v6_replica: OnceLock<crate::v6_replica::ReplicaTarget<RT>>,
    opts: Opts,
    /// Set when `opts` is for an RDS endpoint and has no password.
    rds_iam_auth: Option<RdsIamAuth>,
    use_prepared_statements: bool,
    /// Limits the total number of connections that can be handed out
    /// simultaneously.
    semaphore: Semaphore,
    /// Number of connections that exist, whether idle or checked out. Unlike
    /// the permits held on `semaphore`, this excludes connections that are
    /// still being created.
    established: Arc<AtomicUsize>,
    /// Number of `acquire` calls currently waiting on `semaphore`.
    waiters: AtomicUsize,
    /// Shared with the idle worker, which closes connections idle for longer
    /// than [`MYSQL_INACTIVE_CONNECTION_LIFETIME`].
    connections: Arc<Mutex<IdleConnections>>,
    /// Wakes the idle worker when a returned connection must be closed before
    /// [`IdleConnections::next_cleanup`].
    idle_worker_wakeup: Arc<Notify>,
    stats: ConnectionPoolStats,
    // Used for metrics
    cluster_name: String,
    idle_worker: JoinHandle<()>,
    _runtime: PhantomData<fn() -> RT>,
}

// Deriving the cluster name from the URL is a bit hacky, but seems cleaner than
// to pass cluster_name from 7 layers deep just for metric. It is easy to
// confuse those with the url and db_name that are used in the actual queries.
fn derive_cluster_name(url: &Url) -> &str {
    if url.host_str().is_some_and(|s| s.ends_with(".psdb.cloud")) {
        return url.path().trim_start_matches('/');
    }
    let mut cluster_name = url
        .host_str()
        .and_then(|host| host.split('.').next())
        .unwrap_or("");
    if let Some(name) = cluster_name.strip_suffix("-proxy") {
        cluster_name = name;
    }
    cluster_name
}

impl<RT: Runtime> ConvexMySqlPool<RT> {
    pub fn new(
        url: &Url,
        use_prepared_statements: bool,
        require_leader: bool,
    ) -> anyhow::Result<Self> {
        let cluster_name = derive_cluster_name(url).to_owned();
        let opts = Opts::from_str(url.as_ref())?;
        let ssl_opts = opts.ssl_opts().cloned();
        let use_rds_iam_auth = opts.pass().is_none()
            && aws_utils::rds::region_from_rds_hostname(opts.ip_or_hostname()).is_some();
        let mut opts = OptsBuilder::from_opts(opts);
        if use_rds_iam_auth {
            // RDS only accepts IAM auth tokens through `mysql_clear_password`.
            opts = opts.enable_cleartext_plugin(true);
        }
        if require_leader {
            opts = opts.after_connect(|conn| {
                async move {
                    let readonly: Option<(bool,)> = conn
                        .query_first("SELECT @@global.innodb_read_only OR @@global.read_only")
                        .await?;
                    let Some((readonly,)) = readonly else {
                        return Err(mysql_async::Error::Other("expected a result".into()));
                    };
                    if readonly {
                        return Err(mysql_async::Error::Other(
                            database_operational_error(anyhow::anyhow!(
                                "Connected to a read-only database"
                            ))
                            .into(),
                        ));
                    }
                    Ok(())
                }
                .boxed()
            });
        }
        // The MYSQL_CA_FILE environment variable implicitly enables TLS unless
        // the URL specifies require_ssl=false
        if let Some(ca_file_path) = env::var_os("MYSQL_CA_FILE")
            && !ca_file_path.is_empty()
            && !url
                .query_pairs()
                .any(|(k, v)| k == "require_ssl" && v == "false")
        {
            let ca_file_path = PathBuf::from(ca_file_path);
            anyhow::ensure!(
                ca_file_path.exists(),
                "MYSQL_CA_FILE does not exist: {}",
                ca_file_path.display()
            );
            let ssl_opts = ssl_opts
                .unwrap_or_default()
                .with_root_certs(vec![ca_file_path.into()]);
            opts = opts.ssl_opts(ssl_opts);
        }
        let opts = Opts::from(opts);
        let rds_iam_auth = if use_rds_iam_auth {
            let user = opts
                .user()
                .context("RDS IAM auth requires a username in the MySQL URL")?;
            anyhow::ensure!(
                opts.ssl_opts().is_some(),
                "RDS IAM auth requires TLS: set require_ssl=true in the MySQL URL or set \
                 MYSQL_CA_FILE"
            );
            tracing::info!("Connecting to {cluster_name} with RDS IAM auth as {user}");
            Some(RdsIamAuth {
                cached_opts: Mutex::new(None),
            })
        } else {
            None
        };
        let connections = Arc::new(Mutex::new(IdleConnections {
            conns: Vec::new(),
            // The idle worker's first cleanup runs as soon as it is spawned.
            next_cleanup: Instant::now(),
        }));
        let idle_worker_wakeup = Arc::new(Notify::new());
        let idle_worker = tokio_spawn(
            "mysql_idle_worker",
            idle_worker(
                Arc::downgrade(&connections),
                idle_worker_wakeup.clone(),
                cluster_name.clone(),
            ),
        );
        Ok(Self {
            v6_replica: OnceLock::new(),
            opts,
            rds_iam_auth,
            use_prepared_statements,
            semaphore: Semaphore::new(*MYSQL_MAX_CONNECTIONS),
            established: Arc::new(AtomicUsize::new(0)),
            waiters: AtomicUsize::new(0),
            connections,
            idle_worker_wakeup,
            stats: new_connection_pool_stats(cluster_name.as_str()),
            cluster_name,
            idle_worker,
            _runtime: PhantomData,
        })
    }

    /// Connects to the cluster leader using a Big Brain database URL.
    pub fn connect_cluster(cluster_url: &str, require_ssl: bool) -> anyhow::Result<(Self, String)> {
        let mut url: Url = cluster_url.parse().context("invalid MySQL cluster URL")?;
        anyhow::ensure!(
            !url.username().is_empty(),
            "MySQL cluster URL username must be set"
        );
        let db_name = url.path().trim_start_matches('/').to_string();
        anyhow::ensure!(
            !db_name.is_empty(),
            "MySQL cluster URL must contain a database name"
        );
        if require_ssl {
            url.query_pairs_mut().append_pair("require_ssl", "true");
        }
        url.query_pairs_mut()
            .append_pair("enable_cleartext_plugin", "true");
        let pool = Self::new(
            &url,
            *DATABASE_USE_PREPARED_STATEMENTS,
            true, /* require_leader */
        )?;
        Ok((pool, db_name))
    }

    /// Assumes that we already have a semaphore permit
    async fn get_connection_internal(&self) -> anyhow::Result<PooledConnection> {
        {
            let mut idle = self.connections.lock();
            // Always reuse the newest connection
            while let Some(conn) = idle.conns.pop() {
                if conn.is_expired() {
                    log_discarded_connection(&self.cluster_name, DiscardReason::Expired);
                    continue;
                }
                return Ok(conn);
            }
        }
        // box this since it is less common
        Box::pin(self.create_connection()).await
    }

    #[fastrace::trace]
    async fn create_connection(&self) -> anyhow::Result<PooledConnection> {
        // A guard also counts attempts abandoned by a caller's timeout, such
        // as `acquire`'s, which expires before this one after waiting for a
        // permit.
        let failed = scopeguard::guard((), |()| log_connection_create_failed(&self.cluster_name));
        let mut opts = self.opts.clone();
        if let Some(rds_iam_auth) = &self.rds_iam_auth {
            opts = rds_iam_auth
                .authed_opts(&self.opts)
                .await
                .map_err(database_operational_error)?;
        }
        let conn = with_timeout(Conn::new(opts)).await?;
        ScopeGuard::into_inner(failed);
        Ok(PooledConnection::new(conn, &self.established))
    }

    pub(crate) async fn acquire<'a>(
        &'a self,
        name: &'static str,
        db_name: &'a str,
    ) -> anyhow::Result<MySqlConnection<'a, RT>> {
        let pool_get_timer = get_connection_timer(&self.cluster_name);
        let conn = with_anyhow_timeout(async {
            let permit = {
                struct WaiterGuard<'a>(&'a AtomicUsize);
                impl Drop for WaiterGuard<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::Relaxed);
                    }
                }
                self.waiters.fetch_add(1, Ordering::Relaxed);
                let _waiting = WaiterGuard(&self.waiters);
                self.semaphore
                    .acquire()
                    .trace_if_pending("mysql_semaphore_acquire")
                    .await
                    .context("ConvexMySqlPool has been shut down")
                    .map_err(database_operational_error)?
            };
            let conn = self.get_connection_internal().await?;
            anyhow::Ok((permit, conn))
        })
        .trace_if_pending(func_path!()) // only trace if slow
        .await;
        pool_get_timer.finish(conn.is_ok());
        let (permit, conn) = conn?;
        Ok(MySqlConnection {
            pool: self,
            _permit: permit,
            conn: Some(conn),
            dirty: false,
            labels: vec![
                StaticMetricLabel::new("name", name),
                StaticMetricLabel::new("cluster_name", self.cluster_name.clone()),
            ],
            db_name,
            _tracker: ConnectionTracker::new(&self.stats),
            _timer: connection_lifetime_timer(name, &self.cluster_name),
        })
    }

    /// Runs a read-only query that returns one or zero rows, retrying on
    /// operational errors.
    ///
    /// The statement runs against the URL's default database and must not
    /// use `@db_name`. `name` is used for metrics.
    ///
    /// # Panics
    ///
    /// With prepared statements disabled, only `NULL`, `Bytes`, `Int`, and
    /// `UInt` parameters are supported. `Float`, `Double`, `Date`, and `Time`
    /// parameters panic.
    pub async fn query_optional(
        &self,
        name: &'static str,
        statement: &str,
        params: Vec<MySqlValue>,
    ) -> anyhow::Result<Option<Row>> {
        self.acquire(name, "")
            .await?
            .query_optional(statement, params)
            .await
    }

    /// Runs a read-only query and maps each row with `f`, retrying on
    /// operational errors.
    ///
    /// The statement runs against the URL's default database and must not
    /// use `@db_name`. `name` is used for metrics.
    ///
    /// # Panics
    ///
    /// With prepared statements disabled, only `NULL`, `Bytes`, `Int`, and
    /// `UInt` parameters are supported. `Float`, `Double`, `Date`, and `Time`
    /// parameters panic.
    pub async fn query_collect<R: Send>(
        &self,
        name: &'static str,
        statement: &str,
        params: Vec<MySqlValue>,
        f: impl Fn(Row) -> anyhow::Result<R> + Send + Sync + 'static,
    ) -> anyhow::Result<Vec<R>> {
        self.acquire(name, "")
            .await?
            .query_collect(statement, params, 0 /* size_hint */, f)
            .await
    }

    /// Executes one or more statements, separated by semicolons, using the
    /// text protocol.
    ///
    /// The statements run against the URL's default database and must not
    /// use `@db_name` or `?`. `name` is used for metrics.
    pub async fn execute(&self, name: &'static str, statement: &str) -> anyhow::Result<()> {
        self.acquire(name, "").await?.execute_many(statement).await
    }

    fn return_connection(&self, mut conn: PooledConnection) {
        if conn.is_expired() {
            log_discarded_connection(&self.cluster_name, DiscardReason::Expired);
            return;
        }
        conn.last_used = Instant::now();
        let mut idle = self.connections.lock();
        // don't return connections to a closed pool
        if !self.semaphore.is_closed() {
            let deadline = conn.idle_deadline();
            idle.conns.push(conn);
            log_connection_returned_to_pool(&self.cluster_name);
            if deadline < idle.next_cleanup {
                idle.next_cleanup = deadline;
                self.idle_worker_wakeup.notify_one();
            }
        }
    }

    pub fn cluster_name(&self) -> &str {
        &self.cluster_name
    }

    /// Set before opening deployments: each writer keeps its initial target
    /// for its lifetime.
    pub fn set_v6_replica(&self, target: crate::V6ReplicaTarget<RT>) -> anyhow::Result<()> {
        self.v6_replica
            .set(target)
            .ok()
            .context("V6 replica target is already set")
    }

    /// Report gauges with information about the MySQL pool.
    /// Note that this only makes sense if there is a single pool for this
    /// cluster in this process.
    pub fn log_pool_metrics(&self) {
        let idle = self.connections.lock().conns.len();
        crate::metrics::log_pool_metrics(
            &self.cluster_name,
            idle,
            self.established.load(Ordering::Relaxed),
            self.waiters.load(Ordering::Relaxed),
        );
    }

    /// Disconnects all pooled connections and makes further `acquire` calls
    /// fail.
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down ConvexMySqlPool");
        // N.B.: this doesn't close in-use connections, but they won't be
        // returned to the pool on drop
        self.semaphore.close();
        self.idle_worker.abort();
        let connections = mem::take(&mut self.connections.lock().conns);
        future::try_join_all(connections.into_iter().map(|c| c.conn.disconnect())).await?;
        Ok(())
    }
}

impl<RT: Runtime> Drop for ConvexMySqlPool<RT> {
    fn drop(&mut self) {
        // Dropping the idle connections disconnects them in the background.
        tracing::info!("ConvexMySqlPool dropped");
        self.idle_worker.abort();
    }
}

async fn idle_worker(
    connections: Weak<Mutex<IdleConnections>>,
    wakeup: Arc<Notify>,
    cluster_name: String,
) {
    loop {
        let next_cleanup = if let Some(connections) = connections.upgrade() {
            cleanup_connections(&connections, &cluster_name)
        } else {
            break;
        };
        // Either outcome means it is time to clean up again.
        let _ = tokio::time::timeout_at(next_cleanup, wakeup.notified()).await;
    }
}

/// Closes connections that are idle for too long or past their maximum
/// lifetime. Schedules and returns the next cleanup, for when the next
/// remaining connection needs to be closed.
fn cleanup_connections(connections: &Mutex<IdleConnections>, cluster_name: &str) -> Instant {
    let mut idle = connections.lock();
    idle.conns.retain(|c| {
        let reason = if c.is_expired() {
            DiscardReason::Expired
        } else if c.last_used.elapsed() >= *MYSQL_INACTIVE_CONNECTION_LIFETIME {
            DiscardReason::Idle
        } else {
            return true;
        };
        log_discarded_connection(cluster_name, reason);
        false
    });
    idle.next_cleanup = idle
        .conns
        .iter()
        .map(PooledConnection::idle_deadline)
        .min()
        .unwrap_or_else(|| Instant::now() + *MYSQL_INACTIVE_CONNECTION_LIFETIME);
    idle.next_cleanup
}
