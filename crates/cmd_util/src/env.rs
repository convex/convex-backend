use std::{
    env,
    fmt::Debug,
    fs::File,
    io,
    str::FromStr,
    sync::LazyLock,
};

use sentry_tracing::EventFilter;
use tracing::Level;
use tracing_subscriber::{
    filter::LevelFilter,
    fmt::{
        format::format,
        MakeWriter,
    },
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

pub fn env_config<T>(name: &str, default: T) -> T
where
    T: Debug + FromStr + PartialEq,
    <T as FromStr>::Err: Debug,
{
    let var_s = match env::var(name) {
        Ok(s) => s,
        Err(env::VarError::NotPresent) => return default,
        Err(env::VarError::NotUnicode(..)) => {
            tracing::warn!("Invalid value for {name}, falling back to {default:?}.");
            return default;
        },
    };
    match T::from_str(&var_s) {
        Ok(v) => {
            if v != default {
                tracing::info!("Overriding {name} to {v:?} from environment");
            }
            v
        },
        Err(e) => {
            tracing::warn!("Invalid value {var_s} for {name}, falling back to {default:?}: {e:?}");
            default
        },
    }
}

pub static CONVEX_TRACE_FILE: LazyLock<Option<File>> = LazyLock::new(|| {
    if env::var("CONVEX_TRACE_FILE").is_err() {
        return None;
    }

    let exe_path = std::env::current_exe().expect("Couldn't find exe name");
    let exe_name = exe_path
        .file_name()
        .expect("Path was empty")
        .to_str()
        .expect("Not valid unicode");
    // e.g. `backend.log`
    let filename = format!("{exe_name}.log");

    let file =
        File::create(&filename).unwrap_or_else(|_| panic!("Could not create file {filename}"));
    Some(file)
});

/// Guard object. Hold onto it for as long as you'd like to keep tracing to a
/// file specified by `CONVEX_TRACE_FILE`
pub struct TracingGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Call this from scripts at startup.
#[must_use]
pub fn config_tool() -> TracingGuard {
    config_tracing(io::stderr, Level::ERROR)
}

/// Call this from services at startup.
#[must_use]
pub fn config_service() -> TracingGuard {
    config_tracing(io::stdout, Level::INFO)
}

fn config_tracing<W>(writer: W, level: Level) -> TracingGuard
where
    W: Send + Sync + for<'writer> MakeWriter<'writer> + 'static,
{
    let mut layers = Vec::new();
    let color_disabled = std::env::var("NO_COLOR").is_ok();
    let format_layer = tracing_subscriber::fmt::layer()
        .with_ansi(!color_disabled)
        .with_writer(writer);
    let format_layer = match std::env::var("LOG_FORMAT") {
        Ok(s) if s == "json" => format_layer.event_format(format().json()).boxed(),
        Ok(s) if s == "compact" => format_layer.event_format(format().compact()).boxed(),
        Ok(s) if s == "pretty" => format_layer.event_format(format().pretty()).boxed(),
        _ => format_layer.event_format(format().compact()).boxed(),
    };
    let format_layer = format_layer
        .with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or(EnvFilter::new(level.as_str())),
        )
        .boxed();
    layers.push(format_layer);
    let sentry_layer = sentry_tracing::layer()
        .event_filter(|md| match md.level() {
            &tracing::Level::DEBUG | &tracing::Level::TRACE => EventFilter::Ignore,
            _ => EventFilter::Breadcrumb,
        })
        .span_filter(|_md| false)
        .with_filter(LevelFilter::INFO);
    layers.push(sentry_layer.boxed());

    let guard = if let Some(ref file) = *CONVEX_TRACE_FILE {
        let (file_writer, guard) = tracing_appender::non_blocking(file);
        let file_writer_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_writer)
            .with_filter(
                EnvFilter::from_default_env()
                    .add_directive(Level::INFO.into())
                    .add_directive("common::errors=debug".parse().unwrap()),
            )
            .boxed();
        layers.push(file_writer_layer);
        Some(guard)
    } else {
        None
    };
    tracing_subscriber::registry().with(layers).init();

    TracingGuard { _guard: guard }
}

pub fn config_test() {
    // Try to initialize tracing_subcriber. Ok if it fails - probably
    // means it was initialized already. Ok to be non-rigorous here, because
    // it's very hard to run initialization of logging in tests, so we tend to
    // toss it in common helper methods all over.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .compact()
        .try_init();
}
