use std::{
    env,
    io::ErrorKind,
    mem,
    path::PathBuf,
    str::FromStr,
    sync::OnceLock,
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
    pin_mut,
    select_biased,
    Future,
    FutureExt as _,
    Stream,
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
    Pool,
    PoolConstraints,
    PoolOpts,
    Row,
    TxOpts,
    Value as MySqlValue,
};
use prometheus::VMHistogramVec;
use tokio::time::sleep;
use url::Url;

use crate::metrics::{
    begin_transaction_timer,
    connection_lifetime_timer,
    get_connection_timer,
    log_execute,
    log_large_statement,
    log_query,
    log_query_result,
    log_transaction,
    new_connection_pool_stats,
    query_progress_counter,
    LARGE_STATEMENT_THRESHOLD,
};

fn classify_mysql_error(e: mysql_async::Error) -> anyhow::Error {
    match e {
        mysql_async::Error::Driver(
            DriverError::PoolDisconnected | DriverError::ConnectionClosed,
        )
        | mysql_async::Error::Server(mysql_async::ServerError {
            // Expected operational Vitess errors:
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
        }) if message.contains("primary is not serving")
            || message.contains("for tx killer rollback")
            || message.contains("connection pool timed out")
            || message.contains("connection timed out") =>
        {
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

/// Recognizes the Vitess error returned when a result exceeds its 64 MiB
/// message limit, allowing callers to retry with a smaller page.
pub(crate) fn is_message_too_large_error(
    error: &anyhow::Error,
) -> Option<&mysql_async::ServerError> {
    error
        .chain()
        .find_map(|error| error.downcast_ref::<mysql_async::ServerError>())
        .filter(|database_error| {
            database_error.state == "HY000"
                && database_error.code == 1105
                && database_error.message.contains("message larger than max")
        })
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
    select_biased! {
        r = f.fuse() => {
            r.map_err(classify_mysql_error)
        },
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

pub(crate) struct MySqlConnection<'a, RT: Runtime> {
    conn: Conn,
    labels: Vec<StaticMetricLabel>,
    pool: &'a ConvexMySqlPool<RT>,
    db_name: &'a str,
    _tracker: ConnectionTracker,
    _timer: Timer<VMHistogramVec>,
}

async fn handle_errors_with_retries<R, RT: Runtime>(
    conn: &mut Conn,
    pool: &ConvexMySqlPool<RT>,
    mut f: impl AsyncFnMut(&mut Conn) -> anyhow::Result<R>,
    max_retries: u32,
) -> anyhow::Result<R> {
    let mut attempt = 0;
    loop {
        let (e, should_retry) = match f(conn).await {
            Err(e) if e.is::<DatabaseOperationalError>() => (e, attempt < max_retries),
            Err(e) if e.is::<DatabaseTimeoutError>() => {
                // Don't retry here as we want the caller to receive some
                // backpressure.
                // The mysql protocol doesn't support cancellation, so if a
                // query times out on the client, the connection can't be reused
                // until the server responds to the query.
                // So don't return the connection to the pool.
                (e, false)
            },
            r => return r,
        };
        if should_retry {
            tracing::warn!("Retrying after MySQL error: {e:#}")
        } else {
            tracing::warn!("Discarding connection after MySQL error: {e:#}")
        }
        let old_conn = mem::replace(conn, pool.acquire_internal().await?);
        if should_retry {
            if let Err(e) = old_conn.disconnect().await {
                tracing::warn!("Error disconnecting MySQL connection: {e}");
            }
            attempt += 1;
            continue;
        } else {
            tokio_spawn("disconnect_mysql_conn", async move {
                // Disconnecting the connection could take a long time as well,
                // so do it in the background.
                if let Err(e) = old_conn.disconnect().await {
                    tracing::warn!("Error disconnecting MySQL connection: {e}");
                }
            });
            return Err(e);
        }
    }
}

async fn handle_errors<R, RT: Runtime>(
    conn: &mut Conn,
    pool: &ConvexMySqlPool<RT>,
    f: impl AsyncFnOnce(&mut Conn) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let mut f = Some(f);
    handle_errors_with_retries(
        conn,
        pool,
        async move |conn| f.take().expect("should never retry")(conn).await,
        0, /* max_retries */
    )
    .await
}

impl<RT: Runtime> MySqlConnection<'_, RT> {
    /// Executes multiple statements, separated by semicolons.
    #[fastrace::trace]
    pub async fn execute_many(&mut self, query: &str) -> anyhow::Result<()> {
        log_execute(self.labels.clone());
        let statement = format_mysql_text_protocol(self.db_name, query, vec![], &self.labels)?;
        handle_errors(&mut self.conn, self.pool, async move |conn| {
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
        log_query(self.labels.clone());
        let row = if self.pool.use_prepared_statements {
            let statement = format_mysql_binary_protocol(self.db_name, statement)?;
            handle_errors_with_retries(
                &mut self.conn,
                self.pool,
                async move |conn| with_timeout(conn.exec_first(&statement, params.clone())).await,
                *MYSQL_MAX_QUERY_RETRIES,
            )
            .await?
        } else {
            let statement =
                format_mysql_text_protocol(self.db_name, statement, params, &self.labels)?;
            handle_errors_with_retries(
                &mut self.conn,
                self.pool,
                async move |conn| with_timeout(conn.query_first(&statement)).await,
                *MYSQL_MAX_QUERY_RETRIES,
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
            assert_send(handle_errors_with_retries(
                &mut self.conn,
                self.pool,
                async move |conn| {
                    // Any error or dropped stream after this point leaves the connection
                    // open with MySQL sending data into it. In the worst case, the data
                    // will be consumed & dropped by the *next* client.acquire(), which can
                    // make it hard to attribute latency. Therefore we start a progress
                    // counter that will log if the stream is dropped before being consumed.
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
            assert_send(handle_errors_with_retries(
                &mut self.conn,
                self.pool,
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
            handle_errors(&mut self.conn, self.pool, async move |conn| {
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
            handle_errors(&mut self.conn, self.pool, async move |conn| {
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
        let inner = with_timeout(self.conn.start_transaction(tx_opts)).await?;
        timer.finish();
        Ok(MySqlTransaction {
            inner,
            use_prepared_statements: self.pool.use_prepared_statements,
            db_name: self.db_name,
            labels: &self.labels,
        })
    }

    pub async fn handle_errors<R>(&mut self, r: anyhow::Result<R>) -> anyhow::Result<R> {
        handle_errors(&mut self.conn, self.pool, async move |_| r).await
    }
}

pub(crate) struct MySqlTransaction<'a> {
    inner: mysql_async::Transaction<'a>,
    use_prepared_statements: bool,
    db_name: &'a str,
    labels: &'a [StaticMetricLabel],
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
        Ok(())
    }
}

pub struct ConvexMySqlPool<RT: Runtime> {
    pub(crate) v6_replica: OnceLock<crate::v6_replica::ReplicaTarget<RT>>,
    pool: Pool,
    use_prepared_statements: bool,
    runtime: Option<RT>,
    stats: ConnectionPoolStats,
    // Used for metrics
    cluster_name: String,
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
        runtime: Option<RT>,
    ) -> anyhow::Result<Self> {
        let cluster_name = derive_cluster_name(url).to_owned();
        // NOTE: the inactive_connection_ttl only applies to connections > min
        // constraint. So to make it apply to all connections, set min=0.
        // Connections are accessed in FIFO order from the pool (not round robin)
        // so the pool should be kept small by limiting inactive_connection_ttl.
        let constraints = PoolConstraints::new(0, *MYSQL_MAX_CONNECTIONS).unwrap();
        let pool_opts = PoolOpts::new()
            .with_constraints(constraints)
            // Jitter max connection lifetime with 20%. This is split between
            // the ttl_check_interval and the per-connection jitter.
            .with_ttl_check_interval(*MYSQL_MAX_CONNECTION_LIFETIME / 10)
            .with_inactive_connection_ttl(*MYSQL_INACTIVE_CONNECTION_LIFETIME)
            .with_abs_conn_ttl(Some(*MYSQL_MAX_CONNECTION_LIFETIME))
            .with_abs_conn_ttl_jitter(Some(*MYSQL_MAX_CONNECTION_LIFETIME / 10))
            .with_reset_connection(false); // persist prepared statements
        let opts = Opts::from_str(url.as_ref())?;
        let ssl_opts = opts.ssl_opts().cloned();
        let mut opts = OptsBuilder::from_opts(opts).pool_opts(pool_opts);
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
        Ok(Self {
            v6_replica: OnceLock::new(),
            pool: Pool::new(opts),
            use_prepared_statements,
            runtime,
            stats: new_connection_pool_stats(cluster_name.as_str()),
            cluster_name,
        })
    }

    pub(crate) async fn acquire_internal(&self) -> anyhow::Result<Conn> {
        let pool_get_timer = get_connection_timer(&self.cluster_name);
        let conn = with_timeout(self.pool.get_conn())
            .trace_if_pending(func_path!()) // only trace if slow
            .await;
        pool_get_timer.finish(conn.is_ok());
        conn
    }

    pub(crate) async fn acquire<'a>(
        &'a self,
        name: &'static str,
        db_name: &'a str,
    ) -> anyhow::Result<MySqlConnection<'a, RT>> {
        let conn = self.acquire_internal().await?;
        Ok(MySqlConnection {
            conn,
            labels: vec![
                StaticMetricLabel::new("name", name),
                StaticMetricLabel::new("cluster_name", self.cluster_name.clone()),
            ],
            pool: self,
            db_name,
            _tracker: ConnectionTracker::new(&self.stats),
            _timer: connection_lifetime_timer(name, &self.cluster_name),
        })
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
        crate::metrics::log_pool_metrics(&self.cluster_name, &self.pool.metrics());
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down ConvexMySqlPool");
        Ok(self.pool.clone().disconnect().await?)
    }
}

impl<RT: Runtime> Drop for ConvexMySqlPool<RT> {
    fn drop(&mut self) {
        tracing::info!("ConvexMySqlPool dropped");
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let pool = self.pool.clone();
        runtime.spawn_background("mysql_pool_disconnect", async move {
            let _ = pool.disconnect().await;
            tracing::info!("ConvexMySqlPool pool successfully closed");
        });
    }
}
